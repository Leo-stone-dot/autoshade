# HTTP 设置行为验收

日期：2026-11-23。范围是本地 Web Settings API，不外推到原生 GUI 表单。

## 现有保护与本轮缺口

Store 用例已验证单进程的设置往返、局部 merge 和 Unix 文件权限；
它没有经过 HTTP 的密钥隐藏、空输入语义、端点归属、请求并发和错误响应。
下表均通过真实 `autoshade serve`，函数名只用于定位。

| 状态 | 行为与风险 | 前提和公开断言 | 故障反例 |
| --- | --- | --- | --- |
| [x] | GET 不泄露保存的密钥；`api_settings_get` | 经 POST 保存两类可辨识密钥；GET 只返回 `key_present`，正文不含密钥或密钥字段 | 分别将 analysis / image 密钥值加入 GET |
| [x] | 空密钥表示保留；`api_settings_post` | 已保存密钥后用空值更新模型；模型改变、两类密钥及其端点归属保持 | 分别把两类空字符串当清除 |
| [x] | 密钥不能被静默转发到新端点 | 保存端点 A 和密钥；只改为端点 B 时 GET 显示 key 不可用，文件仍保留密钥及 A；改回 A 后重新可用 | 忽略端点归属；端点改变时分别销毁两类密钥 |
| [x] | 并发局部更新不互相覆盖 | 两个 HTTP 请求同时分别更新 analysis / image 模型；两个 200，GET 和落盘文件同时保留两项 | 每项更新清空另一项，保证无论完成顺序都丢失一个结果 |
| [x] | 设置保存失败不破坏旧配置 | 保存基线后将设置目录设为只读，再 POST；500，旧文件字节和 GET 结果不变；恢复权限 | 失败前截断旧文件；错误报告成功 |

## 隔离与运行

- 每例使用独立 HOME、cwd、数据目录和真实服务子进程。
- 密钥是合成标记，不访问任何外部服务。
- 并发由 barrier 同时发出请求，不用 sleep 猜测。
- Unix 保存失败使用目录权限这个真实 I/O 故障；`Drop` 恢复权限，
  不依赖生产临时文件名。
- 文件断言使用成功 POST 返回的 `saved` 路径；这是公开响应，不重构 Store 路径。

运行：`cargo test --locked --release --features gui --test serve_behavior http_settings::`。

## 执行证据

本机证据目录：`/private/tmp/autoshade-settings-audit.QcJOOd`。

- `tests/http_settings/mod.rs` 新增 5 条默认用例，真实 HTTP 基线通过。
- `run_mutations.py` 对 5 条逐项建立基线，在隔离副本执行 10 个反例；
  `results.json` 全部为 caught，恢复源码后 5 条基线再次通过。
- `leak-analysis-key` / `leak-image-key` 在响应正文出现合成密钥时失败。
- `blank-analysis-clears-key` / `blank-image-clears-key` 在 GET 的
  `key_present` 变为 false 时失败；文件中的端点归属也在正常路径明确断言。
- `reroute-saved-key` 在端点 B 仍显示 key 可用时失败；
  两个 destroy 反例在落盘密钥变为 null 时失败。
- `partial-updates-overwrite` 不依赖完成顺序：任一请求都清空另一方字段；
  用例在 GET 和文件结果缺少其中一项时失败。
- `failed-publish-truncates-live-file` 在旧文件变为空时失败；
  `failed-publish-reports-success` 在返回 200 而非 500 时失败。
- 每个反例均编译成功并运行唯一指定 case；先前一次不编译的试探未计为 caught，
  修正反例后重新执行完整批次。
- `production-before.json` / `production-after.json`：269 个生产 Rust 文件
  校验值一致。
- 最终 clippy、测试政策与 diff whitespace 检查通过。
- `serve-regression-known-failure-excluded.log` 实际运行 46 条：
  42 通过、4 条显式资源/fixture gate、只过滤已单独复现的并发填充失败用例。
  这不是完整套件通过声明；生成式竞争条件仍保留为红灯。

本页没有证明原生 GUI 设置表单行为，也没有请求任何真实 provider。
后续请求并发与浏览器缓存策略见 `SERVER_RUNTIME_AUDIT.md`。
