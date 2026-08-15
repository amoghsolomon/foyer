package com.amazity.foyer.voice

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import ai.moonshine.voice.TextToSpeech
import ai.moonshine.voice.TranscriberOption
import ai.moonshine.voice.TtsSynthesisResult
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URLEncoder
import java.net.URL
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import kotlinx.coroutines.runInterruptible
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.withContext
import org.json.JSONArray

sealed interface ReadAloudState {
    data object Idle : ReadAloudState
    data class Preparing(val progress: Float) : ReadAloudState
    data object Speaking : ReadAloudState
    data class Error(val message: String) : ReadAloudState
}

internal const val KOKORO_VOICE = "kokoro_af_heart"

/**
 * Continuous on-device Kokoro reading through Moonshine.
 *
 * Two independent synthesizers prepare ordered chunks in parallel. Playback waits for an initial
 * two-chunk cushion, then streams all PCM through one AudioTrack while synthesis continues ahead.
 */
class MoonshineKokoroReadAloud(context: Context) {
    private val appContext = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val _state = MutableStateFlow<ReadAloudState>(ReadAloudState.Idle)
    val state: StateFlow<ReadAloudState> = _state.asStateFlow()

    private val engineLock = Any()
    private var engines: List<TextToSpeech> = emptyList()
    private var speechJob: Job? = null
    @Volatile private var audioTrack: AudioTrack? = null

    fun read(text: String) {
        if (text.isBlank() || speechJob?.isActive == true) return
        speechJob = scope.launch {
            try {
                _state.value = ReadAloudState.Preparing(0f)
                val activeEngines = runInterruptible(Dispatchers.IO) { prepareEngines() }
                currentCoroutineContext().ensureActive()
                val chunks = kokoroChunks(text)
                if (chunks.isEmpty()) return@launch

                _state.value = ReadAloudState.Preparing(1f)
                synthesizeAndPlay(chunks, activeEngines)
                currentCoroutineContext().ensureActive()
                _state.value = ReadAloudState.Idle
            } catch (_: CancellationException) {
                _state.value = ReadAloudState.Idle
            } catch (error: Throwable) {
                _state.value = ReadAloudState.Error(error.readAloudMessage())
            } finally {
                releaseAudioTrack()
                speechJob = null
            }
        }
    }

    fun stop() {
        speechJob?.cancel()
        releaseAudioTrack()
        _state.value = ReadAloudState.Idle
    }

    fun close() {
        val activeJob = speechJob
        activeJob?.cancel()
        releaseAudioTrack()
        scope.cancel()
        CoroutineScope(SupervisorJob() + Dispatchers.IO).launch {
            activeJob?.join()
            closeEngines()
            cancel()
        }
    }

    private fun prepareEngines(): List<TextToSpeech> = synchronized(engineLock) {
        if (engines.isNotEmpty()) return@synchronized engines
        val ttsRoot = File(appContext.filesDir, TTS_DIRECTORY).apply { mkdirs() }
        ensureTtsAssets(ttsRoot) { progress ->
            _state.value = ReadAloudState.Preparing(progress * ASSET_PROGRESS_WEIGHT)
        }
        currentCoroutineContextBlocking()

        val loaded = mutableListOf<TextToSpeech>()
        repeat(SYNTHESIS_WORKERS) { index ->
            currentCoroutineContextBlocking()
            try {
                loaded += TextToSpeech(
                    TTS_LANGUAGE,
                    ttsRoot.absolutePath,
                    listOf(TranscriberOption("voice", KOKORO_VOICE)),
                )
                _state.value = ReadAloudState.Preparing(
                    ASSET_PROGRESS_WEIGHT +
                        ENGINE_PROGRESS_WEIGHT * (index + 1f) / SYNTHESIS_WORKERS,
                )
            } catch (error: Exception) {
                // A second native Kokoro handle can exceed the budget on lower-memory phones.
                // Keep the first worker usable rather than failing read aloud completely.
                if (loaded.isEmpty()) throw error
                return@repeat
            }
        }
        engines = loaded
        loaded
    }

    private fun closeEngines() = synchronized(engineLock) {
        engines.forEach { engine ->
            runCatching { engine.stop() }
            runCatching { engine.close() }
        }
        engines = emptyList()
    }

    private suspend fun synthesizeAndPlay(
        chunks: List<String>,
        activeEngines: List<TextToSpeech>,
    ) = coroutineScope {
        val tasks = Channel<IndexedValue<String>>(Channel.UNLIMITED)
        val results = List(chunks.size) { CompletableDeferred<TtsSynthesisResult>() }
        val bufferSlots = Semaphore(MAX_BUFFERED_CHUNKS)
        val workers = activeEngines.map { engine ->
            launch(Dispatchers.IO) {
                for (task in tasks) {
                    bufferSlots.acquire()
                    try {
                        currentCoroutineContext().ensureActive()
                        val audio = runInterruptible { engine.synthesize(task.value) }
                        check(audio.samples?.isNotEmpty() == true && audio.sampleRateHz > 0) {
                            "Kokoro produced no audio"
                        }
                        results[task.index].complete(audio)
                    } catch (error: Throwable) {
                        results[task.index].completeExceptionally(error)
                        if (error is CancellationException) throw error
                    }
                }
            }
        }

        chunks.forEachIndexed { index, chunk ->
            tasks.trySend(IndexedValue(index, chunk)).getOrThrow()
        }
        tasks.close()

        try {
            repeat(minOf(INITIAL_BUFFER_CHUNKS, chunks.size)) { index ->
                results[index].await()
            }
            _state.value = ReadAloudState.Speaking
            withContext(Dispatchers.IO) {
                playContinuously(results, bufferSlots)
            }
            workers.joinAll()
        } finally {
            tasks.cancel()
            workers.forEach { it.cancel() }
            results.forEach { if (it.isActive) it.cancel() }
        }
    }

    private suspend fun playContinuously(
        results: List<CompletableDeferred<TtsSynthesisResult>>,
        bufferSlots: Semaphore,
    ) {
        val first = results.first().await()
        val sampleRate = first.sampleRateHz
        val minimumBuffer = AudioTrack.getMinBufferSize(
            sampleRate,
            AudioFormat.CHANNEL_OUT_MONO,
            AudioFormat.ENCODING_PCM_FLOAT,
        )
        check(minimumBuffer > 0) { "Audio playback is unavailable" }
        val track = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                    .setSampleRate(sampleRate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .build(),
            )
            .setBufferSizeInBytes(maxOf(minimumBuffer, PLAYBACK_BUFFER_BYTES))
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()
        check(track.state == AudioTrack.STATE_INITIALIZED) {
            track.release()
            "Audio playback could not be initialized"
        }
        audioTrack = track

        var totalSamples = 0L
        try {
            track.play()
            results.forEachIndexed { index, deferred ->
                currentCoroutineContext().ensureActive()
                val result = if (index == 0) first else deferred.await()
                check(result.sampleRateHz == sampleRate) {
                    "Kokoro changed sample rate between chunks"
                }
                val samples = checkNotNull(result.samples)
                var offset = 0
                while (offset < samples.size) {
                    currentCoroutineContext().ensureActive()
                    val written = track.write(
                        samples,
                        offset,
                        samples.size - offset,
                        AudioTrack.WRITE_BLOCKING,
                    )
                    check(written > 0) { "Audio playback was interrupted" }
                    offset += written
                    totalSamples += written
                }
                bufferSlots.release()
            }
            while (
                currentCoroutineContext().isActive &&
                track.playbackHeadPosition.toLong() < totalSamples - 1
            ) {
                Thread.sleep(8)
            }
        } finally {
            runCatching { track.stop() }
            runCatching { track.release() }
            if (audioTrack === track) audioTrack = null
        }
    }

    private fun releaseAudioTrack() {
        val active = audioTrack ?: return
        audioTrack = null
        runCatching { active.stop() }
        runCatching { active.flush() }
        runCatching { active.release() }
    }

    private fun ensureTtsAssets(ttsRoot: File, onProgress: (Float) -> Unit) {
        val options = listOf(
            TranscriberOption("g2p_root", ttsRoot.absolutePath),
            TranscriberOption("voice", KOKORO_VOICE),
        )
        val dependencies = JSONArray(TextToSpeech.getTtsDependencies(TTS_LANGUAGE, options))
        val missing = buildList {
            for (index in 0 until dependencies.length()) {
                val key = dependencies.optString(index).trim()
                if (key.isNotEmpty() && key.contains('/')) {
                    val destination = safeAssetDestination(ttsRoot, key)
                    if (!destination.isFile || destination.length() == 0L) add(key)
                }
            }
        }.distinct()

        if (missing.isEmpty()) {
            onProgress(1f)
            return
        }

        missing.forEachIndexed { index, key ->
            currentCoroutineContextBlocking()
            val destination = safeAssetDestination(ttsRoot, key)
            destination.parentFile?.mkdirs()
            downloadTtsAsset(key, destination) { downloaded, total ->
                currentCoroutineContextBlocking()
                val fileProgress = if (total > 0L) {
                    (downloaded.toDouble() / total).coerceIn(0.0, 1.0)
                } else {
                    0.0
                }
                onProgress(((index + fileProgress) / missing.size).toFloat())
            }
        }
        onProgress(1f)
    }

    private fun safeAssetDestination(root: File, key: String): File {
        val canonicalRoot = root.canonicalFile
        val destination = File(canonicalRoot, key).canonicalFile
        check(destination.path.startsWith(canonicalRoot.path + File.separator)) {
            "Moonshine returned an invalid TTS asset path"
        }
        return destination
    }

    private fun downloadTtsAsset(
        key: String,
        destination: File,
        onProgress: (downloaded: Long, total: Long) -> Unit,
    ) {
        val temporary = File(destination.parentFile, "${destination.name}.download").also { it.delete() }
        val encodedKey = key.split('/').joinToString("/") { segment ->
            URLEncoder.encode(segment, "UTF-8").replace("+", "%20")
        }
        val connection = (URL("$TTS_CDN_BASE$encodedKey").openConnection() as HttpURLConnection).apply {
            connectTimeout = 30_000
            readTimeout = 180_000
            instanceFollowRedirects = true
            setRequestProperty("User-Agent", "Foyer-Android/1.0")
        }
        try {
            check(connection.responseCode in 200..299) {
                "Read-aloud model download failed (${connection.responseCode})"
            }
            val total = connection.contentLengthLong
            connection.inputStream.buffered().use { input ->
                temporary.outputStream().buffered().use { output ->
                    val buffer = ByteArray(DOWNLOAD_BUFFER_BYTES)
                    var downloaded = 0L
                    var lastReported = 0L
                    while (true) {
                        currentCoroutineContextBlocking()
                        val count = input.read(buffer)
                        if (count < 0) break
                        output.write(buffer, 0, count)
                        downloaded += count
                        if (downloaded - lastReported >= PROGRESS_REPORT_BYTES) {
                            lastReported = downloaded
                            onProgress(downloaded, total)
                        }
                    }
                    onProgress(downloaded, total)
                }
            }
            check(temporary.length() > 0L) { "The downloaded read-aloud asset was empty" }
            moveTtsAssetAtomically(temporary, destination)
        } catch (error: Throwable) {
            temporary.delete()
            throw error
        } finally {
            connection.disconnect()
        }
    }

    private fun Throwable.readAloudMessage(): String = when (this) {
        is java.net.UnknownHostException -> "Connect once to enable read aloud"
        is java.net.SocketTimeoutException -> "The read-aloud model download timed out"
        is IOException -> message ?: "Read aloud could not be installed"
        else -> message ?: "Read aloud stopped unexpectedly"
    }

    private fun currentCoroutineContextBlocking() {
        if (Thread.currentThread().isInterrupted) throw CancellationException("Read aloud cancelled")
    }

    private companion object {
        const val TTS_LANGUAGE = "en_us"
        const val TTS_DIRECTORY = "moonshine/tts"
        const val TTS_CDN_BASE = "https://download.moonshine.ai/tts/"
        const val SYNTHESIS_WORKERS = 2
        const val INITIAL_BUFFER_CHUNKS = 2
        const val MAX_BUFFERED_CHUNKS = 4
        const val PLAYBACK_BUFFER_BYTES = 128 * 1_024
        const val DOWNLOAD_BUFFER_BYTES = 64 * 1_024
        const val PROGRESS_REPORT_BYTES = 256L * 1_024
        const val ASSET_PROGRESS_WEIGHT = 0.90f
        const val ENGINE_PROGRESS_WEIGHT = 0.10f
    }
}

private fun moveTtsAssetAtomically(source: File, destination: File) {
    try {
        Files.move(
            source.toPath(),
            destination.toPath(),
            StandardCopyOption.REPLACE_EXISTING,
            StandardCopyOption.ATOMIC_MOVE,
        )
    } catch (_: java.nio.file.AtomicMoveNotSupportedException) {
        Files.move(source.toPath(), destination.toPath(), StandardCopyOption.REPLACE_EXISTING)
    }
}

/** Large word-safe blocks; sentence punctuation is preserved but does not force a new chunk. */
internal fun kokoroChunks(text: String, maximumLength: Int = 360): List<String> {
    var remaining = text.replace(Regex("\\s+"), " ").trim()
    if (remaining.isEmpty()) return emptyList()
    val chunks = mutableListOf<String>()
    while (remaining.length > maximumLength) {
        val searchEnd = (maximumLength - 1).coerceAtLeast(1)
        val splitAt = remaining.lastIndexOf(' ', startIndex = searchEnd)
            .takeIf { it > maximumLength / 2 }
            ?: maximumLength
        val phrase = remaining.substring(0, splitAt).trim()
        val splitOnWordBoundary = remaining.getOrNull(splitAt)?.isWhitespace() == true
        chunks += if (splitOnWordBoundary && phrase.lastOrNull() !in KOKORO_PAUSE_MARKS) {
            "$phrase,"
        } else {
            phrase
        }
        remaining = remaining.substring(splitAt).trimStart()
    }
    if (remaining.isNotEmpty()) chunks += remaining
    return chunks
}

private val KOKORO_PAUSE_MARKS = setOf('.', ',', ';', ':', '!', '?')
