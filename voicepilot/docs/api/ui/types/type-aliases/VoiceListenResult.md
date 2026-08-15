[**voicepilot-ui-web**](../../README.md)

***

[voicepilot-ui-web](../../README.md) / [types](../README.md) / VoiceListenResult

# Type Alias: VoiceListenResult

> **VoiceListenResult** = \{ `kind`: `"success"`; `route_outcome`: [`RouteTextResult`](RouteTextResult.md); `stopped_by_vad`: `boolean`; `transcription`: `string`; \} \| \{ `kind`: `"no_speech"`; \} \| \{ `kind`: `"timeout"`; `route_outcome`: [`RouteTextResult`](RouteTextResult.md); `transcription`: `string` \| `null`; \} \| \{ `kind`: `"error"`; `message`: `string`; \}

Defined in: types.ts:44
