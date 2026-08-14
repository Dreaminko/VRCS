# 第三方输出 API v1

第三方输出 API 是只读事件接口，供字幕 Overlay、直播工具和本机自动化订阅 VRCS 的识别、翻译与 Chatbox 生命周期。它使用独立监听端口，不开放设置、采集控制、历史记录或 Chatbox 写入能力，也不复用桌面端内部 `/ws` 协议。

## 启用与安全

在“设置 → 系统 → 第三方输出 API”中启用。默认地址是 `127.0.0.1:8767`，默认关闭。设置变化在重启 VRCS 后生效。

- 回环地址可以为不带 `Origin` 的非浏览器客户端关闭 Token 鉴权；浏览器连接始终需要 Token。
- 非回环地址必须开启 Token 鉴权并配置 Token，否则 Core 只拒绝启动该监听器，主服务仍会运行并在设置页报告失败状态。
- Token 保存在系统凭据管理器的 `VRCS/ExternalAPI/token`，不写入 `config.json`；独立 Core 可使用 `VRCS_EXTERNAL_API_TOKEN` 覆盖。
- 第三方 Token 不能访问内部 REST API，内部 session token 也不会授权第三方事件接口。

监听器只提供：

```text
GET /v1/health
GET /v1/capabilities
GET /v1/events        WebSocket upgrade
```

## 连接与鉴权

非浏览器客户端在 WebSocket 握手中发送：

```http
Authorization: Bearer <token>
```

浏览器连接始终需要 Token。浏览器不能设置该 Header，可传入两个 WebSocket 子协议：

```js
const socket = new WebSocket("ws://127.0.0.1:8767/v1/events", [
  "vrcs.events.v1",
  `vrcs.token.${token}`,
]);
```

浏览器使用的 Token 必须能安全放入 HTTP 子协议字段。设置页生成的 Token 符合该要求。API 不接受 query 参数 Token，避免凭据进入 URL 和日志。

## 订阅

连接后服务端先发送 `system.connected`。客户端必须在 5 秒内订阅：

```json
{"type":"subscribe","events":["asr.*","translation.completed","chatbox.sent"]}
```

支持精确事件名、分组通配符 `asr.*`、`translation.*`、`chatbox.*`，以及全部事件 `*`。成功后返回 `system.subscribed`，其中 `payload.events` 是展开后的事件名。再次订阅会替换原订阅。未知模式、无效 JSON 或其他命令返回 `system.error`。

可订阅事件：

- `asr.partial`、`asr.final`、`asr.failed`
- `translation.started`、`translation.partial`、`translation.completed`、`translation.failed`
- `chatbox.sent`

高频音量事件不通过此 API 输出。客户端落后于广播队列时，服务端发送 `system.lagged` 并关闭该连接；其他客户端和事件生产者不受影响。

## 事件信封

所有领域事件和 `system.*` 控制消息使用同一信封：

```json
{
  "api_version": "1.0",
  "event_id": "5db20d5d-a610-4ec6-a18e-0b12e76a8392",
  "type": "translation.completed",
  "timestamp": "2026-08-14T10:20:30.123Z",
  "message_id": "utterance-2e7f...",
  "source": "microphone",
  "payload": {}
}
```

- `event_id` 是每个事件唯一的 UUID v4。
- `timestamp` 是 RFC 3339 UTC 时间。
- `message_id` 关联同一次识别及其自动翻译。云端 partial/final 使用 Provider 会话的 `utterance_id`；本地识别为每段语音生成一次 ID。
- 数据库中的 `subtitle_id` 只出现在相关事件的 `payload` 中，不作为跨安装稳定标识。
- `source` 的领域值为 `speaker`、`microphone` 或 `chatbox`；`system.*` 控制消息使用 `system`。

API `1.x` 会保持已有事件名、信封字段和字段类型兼容。新增可选字段或事件类型只增加次版本；删除或改变既有字段语义需要新的主版本。

## 最小客户端

```js
const token = "<external-api-token>";
const socket = new WebSocket("ws://127.0.0.1:8767/v1/events", [
  "vrcs.events.v1",
  `vrcs.token.${token}`,
]);

socket.addEventListener("open", () => {
  socket.send(JSON.stringify({ type: "subscribe", events: ["asr.final", "translation.*"] }));
});

socket.addEventListener("message", ({ data }) => {
  const event = JSON.parse(data);
  console.log(event.type, event.message_id, event.payload);
});
```
