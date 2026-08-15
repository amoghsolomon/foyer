package com.amazity.foyer.voice

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.SystemClock
import androidx.core.content.ContextCompat
import ai.moonshine.voice.JNI
import ai.moonshine.voice.Transcriber
import ai.moonshine.voice.TranscriptEvent
import ai.moonshine.voice.TranscriptEventListener
import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.util.function.Consumer
import kotlin.math.sqrt
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.runInterruptible
import kotlinx.coroutines.withContext

sealed interface VoiceInputState {
    data object Idle : VoiceInputState
    data class Preparing(val progress: Float) : VoiceInputState
    data class Listening(val levels: List<Float>, val elapsedMillis: Long) : VoiceInputState
    data class Error(val message: String) : VoiceInputState
}

/** Low-latency English dictation using Moonshine Tiny entirely on device. */
class MoonshineVoiceInput(context: Context) {
    private val appContext = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val _state = MutableStateFlow<VoiceInputState>(VoiceInputState.Idle)
    val state: StateFlow<VoiceInputState> = _state.asStateFlow()

    private val transcriptLock = Any()
    private val transcriptLines = linkedMapOf<Long, String>()
    private var transcriber: Transcriber? = null
    private var sessionJob: Job? = null
    @Volatile private var recorder: AudioRecord? = null
    @Volatile private var stopRequested = false
    private var onTranscript: ((String) -> Unit)? = null

    private val transcriptListener = object : TranscriptEventListener() {
        override fun onLineTextChanged(event: TranscriptEvent.LineTextChanged) {
            updateTranscript(event.line.id, event.line.text)
        }

        override fun onLineCompleted(event: TranscriptEvent.LineCompleted) {
            updateTranscript(event.line.id, event.line.text)
        }

        override fun onError(event: TranscriptEvent.Error) {
            scope.launch {
                _state.value = VoiceInputState.Error(
                    event.cause.message ?: "Moonshine transcription stopped unexpectedly",
                )
                stopRequested = true
                requestRecorderStop()
            }
        }
    }
    private val eventConsumer = Consumer<TranscriptEvent> { event ->
        event.accept(transcriptListener)
    }

    fun start(onTranscript: (String) -> Unit) {
        if (sessionJob?.isActive == true) return
        if (
            ContextCompat.checkSelfPermission(appContext, Manifest.permission.RECORD_AUDIO) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            _state.value = VoiceInputState.Error("Microphone permission is required")
            return
        }

        synchronized(transcriptLock) { transcriptLines.clear() }
        this.onTranscript = onTranscript
        stopRequested = false
        sessionJob = scope.launch {
            try {
                _state.value = VoiceInputState.Preparing(0f)
                val activeTranscriber = runInterruptible(Dispatchers.IO) { prepareTranscriber() }
                currentCoroutineContext().ensureActive()
                if (stopRequested) return@launch

                val activeRecorder = createRecorder()
                recorder = activeRecorder
                activeRecorder.startRecording()
                check(activeRecorder.recordingState == AudioRecord.RECORDSTATE_RECORDING) {
                    "The microphone could not start recording"
                }
                activeTranscriber.start()
                val startedAt = SystemClock.elapsedRealtime()
                _state.value = VoiceInputState.Listening(emptyList(), 0L)
                withContext(Dispatchers.IO) {
                    captureAudio(activeTranscriber, activeRecorder, startedAt)
                }
                if (_state.value !is VoiceInputState.Error) _state.value = VoiceInputState.Idle
            } catch (_: CancellationException) {
                _state.value = VoiceInputState.Idle
            } catch (error: Throwable) {
                _state.value = VoiceInputState.Error(error.voiceMessage())
            } finally {
                recorder = null
                this@MoonshineVoiceInput.onTranscript = null
            }
        }
    }

    fun stop() {
        stopRequested = true
        if (_state.value is VoiceInputState.Preparing) {
            sessionJob?.cancel()
            _state.value = VoiceInputState.Idle
        } else {
            requestRecorderStop()
        }
    }

    fun close() {
        stopRequested = true
        requestRecorderStop()
        sessionJob?.cancel()
        transcriber?.removeListener(eventConsumer)
        transcriber = null
        scope.cancel()
    }

    private fun prepareTranscriber(): Transcriber {
        transcriber?.let { return it }
        val modelDirectory = ensureModelFiles { progress ->
            _state.value = VoiceInputState.Preparing(progress)
        }
        return Transcriber().also { loaded ->
            loaded.addListener(eventConsumer)
            loaded.loadFromFiles(modelDirectory.absolutePath, JNI.MOONSHINE_MODEL_ARCH_TINY)
            transcriber = loaded
        }
    }

    @SuppressLint("MissingPermission")
    private fun createRecorder(): AudioRecord {
        val minimum = AudioRecord.getMinBufferSize(
            SAMPLE_RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        check(minimum > 0) { "This device does not support 16 kHz microphone capture" }
        val audioRecord = AudioRecord(
            MediaRecorder.AudioSource.VOICE_RECOGNITION,
            SAMPLE_RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
            maxOf(minimum * 2, SAMPLE_RATE),
        )
        check(audioRecord.state == AudioRecord.STATE_INITIALIZED) {
            audioRecord.release()
            "The microphone could not be initialized"
        }
        return audioRecord
    }

    private suspend fun captureAudio(
        activeTranscriber: Transcriber,
        activeRecorder: AudioRecord,
        startedAt: Long,
    ) {
        val samples = ShortArray(AUDIO_CHUNK_SAMPLES)
        val levels = ArrayDeque<Float>(WAVEFORM_SAMPLES)
        try {
            while (currentCoroutineContext().isActive && !stopRequested) {
                val count = activeRecorder.read(samples, 0, samples.size)
                if (count <= 0) {
                    if (!stopRequested) error("Microphone capture was interrupted")
                    break
                }

                val audio = FloatArray(count)
                var energy = 0.0
                for (index in 0 until count) {
                    val value = samples[index] / 32768f
                    audio[index] = value
                    energy += value * value
                }
                activeTranscriber.addAudio(audio, SAMPLE_RATE)

                val rms = sqrt(energy / count).toFloat()
                val level = (rms * 5.5f).coerceIn(0.035f, 1f)
                if (levels.size == WAVEFORM_SAMPLES) levels.removeFirst()
                levels.addLast(level)
                _state.value = VoiceInputState.Listening(
                    levels = levels.toList(),
                    elapsedMillis = SystemClock.elapsedRealtime() - startedAt,
                )
            }
        } finally {
            withContext(NonCancellable) {
                runCatching {
                    if (activeRecorder.recordingState == AudioRecord.RECORDSTATE_RECORDING) {
                        activeRecorder.stop()
                    }
                }
                activeRecorder.release()
                runCatching { activeTranscriber.stop() }
            }
        }
    }

    private fun updateTranscript(lineId: Long, text: String) {
        val merged = synchronized(transcriptLock) {
            val normalized = text.replace(Regex("\\s+"), " ").trim()
            if (normalized.isBlank()) transcriptLines.remove(lineId) else transcriptLines[lineId] = normalized
            transcriptLines.values.joinToString(" ").trim()
        }
        if (merged.isNotBlank()) {
            scope.launch { onTranscript?.invoke(merged) }
        }
    }

    private fun requestRecorderStop() {
        runCatching {
            recorder?.takeIf { it.recordingState == AudioRecord.RECORDSTATE_RECORDING }?.stop()
        }
    }

    private fun ensureModelFiles(onProgress: (Float) -> Unit): File {
        val directory = File(appContext.filesDir, MODEL_DIRECTORY).apply { mkdirs() }
        val totalBytes = MODEL_FILES.sumOf(ModelFile::size)
        var completedBytes = 0L

        MODEL_FILES.forEach { model ->
            val destination = File(directory, model.name)
            if (destination.isValid(model)) {
                completedBytes += model.size
                onProgress(completedBytes.toFloat() / totalBytes)
                return@forEach
            }
            destination.delete()
            val partial = File(directory, "${model.name}.download").also { it.delete() }
            downloadModel(model, partial, completedBytes, totalBytes, onProgress)
            check(partial.isValid(model)) { "The downloaded ${model.name} failed validation" }
            moveVoiceModelAtomically(partial, destination)
            completedBytes += model.size
            onProgress(completedBytes.toFloat() / totalBytes)
        }

        // Reclaim the superseded Parakeet download after Moonshine is safely installed.
        File(appContext.filesDir, "parakeet").deleteRecursively()
        return directory
    }

    private fun downloadModel(
        model: ModelFile,
        destination: File,
        completedBytes: Long,
        totalBytes: Long,
        onProgress: (Float) -> Unit,
    ) {
        val connection = (URL("$MODEL_BASE_URL/${model.name}").openConnection() as HttpURLConnection).apply {
            connectTimeout = 30_000
            readTimeout = 180_000
            instanceFollowRedirects = true
            setRequestProperty("User-Agent", "Foyer-Android/1.0")
        }
        try {
            check(connection.responseCode in 200..299) {
                "Moonshine model download failed (${connection.responseCode})"
            }
            connection.inputStream.buffered().use { input ->
                FileOutputStream(destination).buffered().use { output ->
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
                            onProgress((completedBytes + downloaded).toFloat() / totalBytes)
                        }
                    }
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun File.isValid(model: ModelFile): Boolean {
        if (!isFile || length() != model.size) return false
        val digest = MessageDigest.getInstance("SHA-256")
        inputStream().buffered().use { input ->
            val buffer = ByteArray(DOWNLOAD_BUFFER_BYTES)
            while (true) {
                currentCoroutineContextBlocking()
                val count = input.read(buffer)
                if (count < 0) break
                digest.update(buffer, 0, count)
            }
        }
        return digest.digest().joinToString("") { "%02x".format(it.toInt() and 0xff) } == model.sha256
    }

    private fun Throwable.voiceMessage(): String = when (this) {
        is java.net.UnknownHostException -> "Connect once to download Moonshine (44 MB)"
        is java.net.SocketTimeoutException -> "The Moonshine download timed out"
        else -> message ?: "Moonshine transcription stopped unexpectedly"
    }

    private fun currentCoroutineContextBlocking() {
        if (Thread.currentThread().isInterrupted) throw CancellationException("Dictation cancelled")
    }

    private data class ModelFile(val name: String, val size: Long, val sha256: String)

    private companion object {
        const val SAMPLE_RATE = 16_000
        const val AUDIO_CHUNK_SAMPLES = 2_048
        const val WAVEFORM_SAMPLES = 28
        const val DOWNLOAD_BUFFER_BYTES = 64 * 1_024
        const val PROGRESS_REPORT_BYTES = 256L * 1_024
        const val MODEL_DIRECTORY = "moonshine/tiny-en-quantized"
        const val MODEL_BASE_URL =
            "https://download.moonshine.ai/model/tiny-en/quantized/tiny-en"
        val MODEL_FILES = listOf(
            ModelFile(
                "encoder_model.ort",
                13_281_600,
                "94e90a4654fc45cdfedb77c4c08e1739f48862998e58fada384b25118134f221",
            ),
            ModelFile(
                "decoder_model_merged.ort",
                30_412_256,
                "cf524c4862d36e9e5ab032eddc73637efd822d70e868ac575cf1a46e1e4708a0",
            ),
            ModelFile(
                "tokenizer.bin",
                249_974,
                "6884b35fd6377d4c4d32336a0bc152f36b64d1e45b6503683cdc238250a8472d",
            ),
        )
    }
}

private fun moveVoiceModelAtomically(source: File, destination: File) {
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
