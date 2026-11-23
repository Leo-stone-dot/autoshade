# 本地服务运行时行为验收

日期：2026-11-23。范围是 `autoshade serve` 的请求并发与浏览器响应策略。

## 行为清单

| 状态 | 行为及故障后果 | 公开前提与断言 | 故障反例 |
| --- | --- | --- | --- |
| [x] | 同时处理的请求有界；突发请求不得无界创建线程/堆叠内存 | 8 个真实 POST 都收到 `100 Continue` 后保持正文未完成；第 9 个 GET 不得返回；释放一项后 GET 应在期限内成功，其余请求随后均可完成 | 把请求上限改为 9 |
| [x] | 请求完成会归还名额并唤醒等待者 | 同上，释放第一项后第 9 个请求成功，不需重启服务 | Drop 不通知等待者 |
| [x] | 旧 recipe 404 不能被浏览器复用覆盖新保存 | 先 GET 未保存 recipe 得 404/no-store，经 POST 保存，再 GET 得新内容/no-store | 动态响应改为可缓存 |
| [x] | 页面不能被第三方 frame 承载 | GET `/` 返回 `X-Frame-Options: DENY` | 删除 frame 拒绝头 |
| [x] | 重启后浏览器不得复用旧 UI 文档 | GET `/` 的 Cache-Control 同时含 `no-cache`、`no-store`、`must-revalidate` | 删除 HTML 缓存策略 |

## 隔离与运行

- 每例使用独立服务进程、动态端口、数据目录和合成照片。
- 请求名额测试使用 HTTP 的 `Expect: 100-continue` 确认 8 个 handler 已进入正文读取；
  第 9 个请求通过 channel 回传结果，前后均有明确 timeout。
- 所有悬挂请求最终发送合法正文并读取最终响应，避免把服务退出清理当通过。
- 不测试线程、私有 Permit 字段或函数调用次数；验收的是客户端可观察的排队和恢复。

运行：`cargo test --locked --release --features gui --test serve_behavior http_runtime::`。

## 执行证据

本机证据目录：`/private/tmp/autoshade-runtime-audit.fxtTKw`。

- `tests/http_runtime/mod.rs` 新增 3 条默认用例，真实 HTTP 基线通过。
- 等待线程使用独立系统线程；异常分支先结束该例自己的服务进程再 join，
  因此“未通知”反例是用例的 5 秒恢复期限失败，不是测试工具永久挂住。
- `run_mutations.py` 逐项建立 3 条基线，在隔离副本执行 5 个反例；
  `results.json` 全部为 caught，恢复源码后 3 条再次通过。
- `admit-ninth-request`：第 9 个 GET 在任一 active POST 完成前返回 200。
- `do-not-wake-waiter`：释放名额后等待请求仍超时；服务随后由测试清理。
- `cache-dynamic-responses`：首次 recipe 404 不再声明 `no-store`。
- `allow-framing`：页面没有 `X-Frame-Options`。
- `cache-ui-document`：页面没有 Cache-Control。
- 每个反例均编译成功并运行唯一指定 case；`production-before.json` /
  `production-after.json` 显示 269 个生产 Rust 文件校验值一致。

本页没有证明 aggregate request-body 字节预算，也没有模拟 handler panic；
不得由“8 个请求有界”外推这两项已受保护。
后续未触达的 Web 公开请求审计见 `PUBLIC_API_BEHAVIOR_AUDIT.md`。
