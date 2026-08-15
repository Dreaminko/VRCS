# 术语表 JSON 与在线订阅

VRCS 使用同一种 JSON 格式导入、导出本地术语表，也可以从公开 HTTP 地址订阅在线术语表。远程地址必须使用 HTTPS；本机回环地址可以使用 HTTP。订阅不支持 Cookie、自定义 Header 或鉴权信息。

“设置 → 翻译”中的本地术语表与在线订阅显示在同一个有序列表中。可以添加多个来源，并分别启用、停用、排序或删除；列表从上到下决定重复术语的匹配优先级。所有已启用来源合计最多使用 500 条术语，超过上限的条目会被忽略。

## JSON 格式

```json
{
  "version": 1,
  "name": "VRChat Community Glossary",
  "entries": [
    {
      "source": "JP Tutorial World",
      "target": null,
      "category": "world",
      "case_sensitive": false
    },
    {
      "source": "instance",
      "target": "实例",
      "category": "game",
      "case_sensitive": false
    }
  ]
}
```

- `version` 必须为 `1`。
- `name` 可省略，最长 100 个字符。
- `entries` 最多包含 500 条术语。
- `source` 必填，`target` 为 `null` 时表示保持原文。
- `category` 可选值为 `person`、`world`、`game`、`custom`，默认 `custom`。
- `case_sensitive` 默认为 `false`。
- `source` 与 `target` 必须是单行文本，各自最长 200 个字符。
- 同一来源内，重复项按 `source` 和 `case_sensitive` 的组合判断；不区分大小写的条目会先将 `source` 转为小写再比较。

本地术语表可以通过编辑弹窗导入该 JSON，也可以导出为文件用于备份、编辑或分享。导入时会严格校验格式和字段，不会自动接受未知结构。

VRCS 会在添加订阅时、应用启动时、手动刷新时和每 24 小时刷新一次。每个订阅独立维护刷新状态和缓存；服务端支持时使用 `ETag` 或 `Last-Modified` 条件请求。刷新失败不会清除该订阅最后一次成功获取的缓存。
