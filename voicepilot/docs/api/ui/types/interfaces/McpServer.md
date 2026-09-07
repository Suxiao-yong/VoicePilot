[**voicepilot-ui-web**](../../README.md)

***

[voicepilot-ui-web](../../README.md) / [types](../README.md) / McpServer

# Interface: McpServer

Defined in: types.ts:139

## Properties

### allowed\_origins

> **allowed\_origins**: `string` \| `null`

Defined in: types.ts:147

***

### allowed\_paths

> **allowed\_paths**: `string` \| `null`

Defined in: types.ts:148

***

### args

> **args**: `string` \| `null`

Defined in: types.ts:152

JSON 字符串数组(如 `["-y","@playwright/mcp@latest"]`)。

***

### command

> **command**: `string` \| `null`

Defined in: types.ts:150

拉起子进程的命令(如 npx);`null` = 进程内 server。

***

### enabled

> **enabled**: `boolean`

Defined in: types.ts:144

***

### env

> **env**: `string` \| `null`

Defined in: types.ts:154

JSON 字符串→字符串对象;值是配置机密,绝不写入审计/日志。

***

### name

> **name**: `string`

Defined in: types.ts:141

***

### protocol\_version

> **protocol\_version**: `string` \| `null`

Defined in: types.ts:146

***

### server\_id

> **server\_id**: `string`

Defined in: types.ts:140

***

### transport

> **transport**: `string`

Defined in: types.ts:143

***

### trusted

> **trusted**: `boolean`

Defined in: types.ts:145

***

### version

> **version**: `string`

Defined in: types.ts:142
