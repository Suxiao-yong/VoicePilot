[**voicepilot-ui-web**](../../README.md)

***

[voicepilot-ui-web](../../README.md) / [api](../README.md) / listDagHistory

# Function: listDagHistory()

> **listDagHistory**(`limit?`, `offset?`, `filter?`): `Promise`\<[`DagPlanSummary`](../../types/interfaces/DagPlanSummary.md)[]\>

Defined in: api.ts:238

分页 + 状态过滤查询 DAG 历史。

## Parameters

### limit?

`number` = `20`

### offset?

`number` = `0`

### filter?

[`DagStatusFilter`](../../types/type-aliases/DagStatusFilter.md) = `"all"`

## Returns

`Promise`\<[`DagPlanSummary`](../../types/interfaces/DagPlanSummary.md)[]\>
