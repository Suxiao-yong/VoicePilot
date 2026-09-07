[**voicepilot-ui-web**](../../README.md)

***

[voicepilot-ui-web](../../README.md) / [api](../README.md) / invokeReloadSkills

# Function: invokeReloadSkills()

> **invokeReloadSkills**(): `Promise`\<[`UserSkill`](../../types/interfaces/UserSkill.md)[]\>

Defined in: api.ts:196

重扫 `%APPDATA%\voicepilot\skills\*.md`,upsert 到 DB,返回当前列表。

## Returns

`Promise`\<[`UserSkill`](../../types/interfaces/UserSkill.md)[]\>
