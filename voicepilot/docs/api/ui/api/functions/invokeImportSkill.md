[**voicepilot-ui-web**](../../README.md)

***

[voicepilot-ui-web](../../README.md) / [api](../README.md) / invokeImportSkill

# Function: invokeImportSkill()

> **invokeImportSkill**(`sourcePath`): `Promise`\<[`UserSkill`](../../types/interfaces/UserSkill.md)\>

Defined in: api.ts:191

校验 + 复制 .md 到 skills 目录 + reload。
source_path 由前端通过 `@tauri-apps/plugin-dialog` 的 `open()` 选择。

## Parameters

### sourcePath

`string`

## Returns

`Promise`\<[`UserSkill`](../../types/interfaces/UserSkill.md)\>
