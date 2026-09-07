[**voicepilot-ui-web**](../../README.md)

***

[voicepilot-ui-web](../../README.md) / [types](../README.md) / SettingsUpdate

# Interface: SettingsUpdate

Defined in: types.ts:104

Wave 3:写入 DTO —— 非 secret 字段 + 显式 secret 操作。
`llm_api_key` 仅在用户输入新 key 时传(空 = 保持现有);`clear_llm_api_key`
为 true 时删除已存储的 key。

## Properties

### clear\_llm\_api\_key

> **clear\_llm\_api\_key**: `boolean`

Defined in: types.ts:122

***

### compensation\_ttl\_hours

> **compensation\_ttl\_hours**: `number`

Defined in: types.ts:114

***

### llm\_api\_key

> **llm\_api\_key**: `string` \| `null`

Defined in: types.ts:121

***

### llm\_base\_url

> **llm\_base\_url**: `string`

Defined in: types.ts:118

***

### llm\_enabled

> **llm\_enabled**: `boolean`

Defined in: types.ts:117

***

### llm\_model

> **llm\_model**: `string`

Defined in: types.ts:119

***

### llm\_provider\_url

> **llm\_provider\_url**: `string`

Defined in: types.ts:120

***

### privacy\_mode

> **privacy\_mode**: `boolean`

Defined in: types.ts:113

***

### tts\_enabled

> **tts\_enabled**: `boolean`

Defined in: types.ts:115

***

### tts\_model\_path

> **tts\_model\_path**: `string`

Defined in: types.ts:116

***

### uia\_allowed\_apps

> **uia\_allowed\_apps**: `string`[]

Defined in: types.ts:123

***

### vad\_energy\_threshold

> **vad\_energy\_threshold**: `number`

Defined in: types.ts:108

***

### vad\_max\_silence\_ms

> **vad\_max\_silence\_ms**: `number`

Defined in: types.ts:109

***

### vad\_min\_speech\_ms

> **vad\_min\_speech\_ms**: `number`

Defined in: types.ts:110

***

### voice\_chunk\_duration\_ms

> **voice\_chunk\_duration\_ms**: `number`

Defined in: types.ts:112

***

### voice\_language

> **voice\_language**: `string` \| `null`

Defined in: types.ts:106

***

### voice\_max\_duration\_ms

> **voice\_max\_duration\_ms**: `number`

Defined in: types.ts:111

***

### voice\_model\_path

> **voice\_model\_path**: `string`

Defined in: types.ts:105

***

### voice\_threads

> **voice\_threads**: `number`

Defined in: types.ts:107
