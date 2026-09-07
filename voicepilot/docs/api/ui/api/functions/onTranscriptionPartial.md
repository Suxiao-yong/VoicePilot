[**voicepilot-ui-web**](../../README.md)

***

[voicepilot-ui-web](../../README.md) / [api](../README.md) / onTranscriptionPartial

# Function: onTranscriptionPartial()

> **onTranscriptionPartial**(`handler`): `Promise`\<`UnlistenFn`\>

Defined in: api.ts:85

Subscribe to `transcription-partial` events(W6b-2 issue #47)。
listen 期间每 2s 发射一次,webview 实时显示 partial 转写。

## Parameters

### handler

(`payload`) => `void`

## Returns

`Promise`\<`UnlistenFn`\>
