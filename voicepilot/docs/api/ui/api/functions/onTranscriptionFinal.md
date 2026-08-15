[**voicepilot-ui-web**](../../README.md)

***

[voicepilot-ui-web](../../README.md) / [api](../README.md) / onTranscriptionFinal

# Function: onTranscriptionFinal()

> **onTranscriptionFinal**(`handler`): `Promise`\<`UnlistenFn`\>

Defined in: api.ts:72

Subscribe to `transcription-final` events emitted by the Rust side.

TODO(W6b-2): MainView currently uses synchronous `voiceListen()` invoke to
get the full result. This listener is reserved for future partial-transcript
streaming support (spec issue #47, deferred to W6b-2).

## Parameters

### handler

(`payload`) => `void`

## Returns

`Promise`\<`UnlistenFn`\>
