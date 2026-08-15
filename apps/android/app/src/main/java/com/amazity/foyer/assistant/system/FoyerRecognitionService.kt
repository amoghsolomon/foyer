package com.amazity.foyer.assistant.system

import android.content.Intent
import android.speech.RecognitionService
import android.speech.SpeechRecognizer

/** Required assistant metadata on Android 16; Foyer performs dictation inside its session UI. */
class FoyerRecognitionService : RecognitionService() {
    override fun onStartListening(recognizerIntent: Intent, listener: Callback) {
        listener.error(SpeechRecognizer.ERROR_CLIENT)
    }

    override fun onStopListening(listener: Callback) = Unit

    override fun onCancel(listener: Callback) = Unit
}
