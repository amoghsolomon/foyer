package com.amazity.foyer.assistant

import android.util.Log
import org.json.JSONObject

object AssistantProtocol {
    private const val ACTION_START = "<foyer-client-action>"
    private const val ACTION_END = "</foyer-client-action>"
    private val actionPattern = Regex(
        "${Regex.escape(ACTION_START)}(.*?)${Regex.escape(ACTION_END)}",
        setOf(RegexOption.DOT_MATCHES_ALL, RegexOption.IGNORE_CASE),
    )

    fun parse(response: JSONObject): AssistantTurn {
        val rawText = response.optJSONObject("result")?.optString("text")
            ?.takeIf(String::isNotBlank)
            ?: response.optString("text")
        return parseText(rawText)
    }

    fun parseText(rawText: String): AssistantTurn {
        val match = actionPattern.find(rawText)
        val action = match?.groupValues?.getOrNull(1)?.let(::parseAction)
        val displayText = actionPattern.replace(rawText, "")
            .replace(Regex("\\n{3,}"), "\n\n")
            .trim()
        return AssistantTurn(text = displayText, action = action)
    }

    private fun parseAction(json: String): ClientAction? = runCatching {
        val payload = JSONObject(json.trim())
        val wireType = payload.getString("type")
        val type = ClientActionType.fromWireName(wireType) ?: run {
            Log.w(TAG, "Ignoring unsupported client action type: ${wireType.take(80)}")
            return@runCatching null
        }
        val rawArguments = payload.optJSONObject("arguments") ?: JSONObject()
        val arguments = buildMap {
            rawArguments.keys().forEach { key ->
                val value = rawArguments.opt(key)
                if (value != null && value != JSONObject.NULL) put(key, value.toString())
            }
        }
        ClientAction(type, arguments)
    }.getOrNull()

    private const val TAG = "AssistantProtocol"
}
