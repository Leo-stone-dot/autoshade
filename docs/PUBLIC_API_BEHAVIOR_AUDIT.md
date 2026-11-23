# Web 公开 API 行为验收

日期：2026-11-23。候选来自可采集服务子进程的诊断报告，
但按公开工作流和故障后果选材，不按未覆盖函数或行数。

## 行为清单

| 状态 | 公开行为 | 前提与断言 | 故障反例 |
| --- | --- | --- | --- |
| [x] | Reset 使用的 fresh base 与首次打开一致，且不随保存编辑漂移 | 未保存 `/api/recipe` 的 404 JSON 与 `/api/fresh-base` 相同；保存曝光后 recipe 改变，但 fresh payload 前后相同 | fresh-base 返回空 payload；初始 404 使用另一 payload |
| [x] | Web 控件声明有效并保留关键公开范围 | `/api/controls` 名称唯一、默认值在范围内；Exposure 为 -5..5/0，Temperature 为 2000..50000/null | 返回空控件；修改 Exposure 范围 |
| [x] | reference library 经 HTTP 成功发布并可查询 | 两张合成 finished photo、外部 embedding 仅替换为本地 fixture；build 返回 total=2，info 报 built/source/file，文件可读 | 成功响应谎报 total；info 隐藏已建 library |
| [x] | library 重建失败保留上一可用版本 | 首次 build 后新增照片并让 embed 外部进程失败；400，info 仍为原 total=2，库文件字节不变 | 错误路径删除旧 library |
| [x] | Analyze 缺密钥时在坏图片解码前拒绝且不保存 | 可枚举但不可解码的 PNG、direction 请求、无密钥；500 明确缺 key，不出现 decode 错误，recipe 仍 404，源文件不变 | 跳过缺密钥前置检查 |
| [x] | Library guide 与 typed direction 冲突是客户错误 | 合法图片，`guide=library` 同时发 guidance；400，源文件和 recipe 不变 | 忽略冲突继续分析 |
| [ ] | Harness Analyze 成功响应与已提交 develop 一致 | 外部 harness 由本地 fixture 替换并在调用时提交 recipe；HTTP 返回同一 recipe、Accept、saved、revision，重启后仍可读，源图不变 | 已捕获 Accept 识别和 revision 丢失；recipe / saved 仍待独立反例 |
| [ ] | Harness 在提交前失败保留已有保存 | 先保存旧 recipe，再令外部 harness 在写入前非零退出；500，旧 recipe/revision 和源图不变 | 已捕获错误退出被当成成功路径；旧保存断言仍待独立反例 |
| [ ] | Harness 部分提交后失败不留下不明状态 | fixture 先改 develop 再非零退出；旧保存应保留，或 API 准确披露已提交结果 | 当前未构造；若现有协议不满足，需要生产修改授权 |

## 隔离

- 每例使用真实服务进程、独立数据目录与合成文件。
- embedding fixture 只替换外部模型进程，真实读取 manifest 并产出固定维度向量；
  fixture 失败是非零退出，不替产品决定 library 状态。
- Analyze 缺密钥用坏图片证明顺序，但断言是用户看到的错误和文件状态，
  不扫描源码、不要求某个 helper 被调用。
- 本批不执行付费模型，也不把“不发起网络”外推成 Analyze 成功路径已覆盖。

运行：`cargo test --locked --release --features gui --test serve_behavior http_public::`。

## 执行证据

本机证据目录：`/private/tmp/autoshade-public-api-audit.N9GSuo`。

- `tests/http_public/mod.rs` 新增 6 条默认用例，真实 HTTP 基线通过。
- library 的 embedding fixture 读取真实 manifest 并输出 768 维合成向量；
  rebuild 失败通过非零进程退出产生，旧 library 状态由产品读取。
- `run_mutations.py` 对 6 条逐项建立基线，执行 9 个可编译反例；
  `results.json` 全部为 caught，恢复源码后 6 条再次通过。
- `empty-fresh-base` / `different-initial-base` 在两个公开 calibration payload
  不一致时失败；保存后的 fresh payload 仍单独断言不漂移。
- `empty-controls` / `wrong-exposure-range` 分别在空 catalogue 和 -4..4 范围失败。
- `wrong-library-total` / `hide-built-library` 在响应事实错误时失败；
  `delete-library-on-rebuild-failure` 在失败后 info 变为 absent 时失败。
- `decode-before-key-check` 返回 PNG 解码错误而非缺 key，命中用户错误文案断言；
  `ignore-guide-conflict` 返回 500 而非客户错误 400。
- 第一次脚本试探因 mutation scope 不含首次 404 的 payload 位置，在执行前的
  唯一性检查中停止；未计作验证。扩大到该唯一字面量后重新执行完整批次。
- `production-before.json` / `production-after.json` 显示 269 个生产 Rust 文件
  校验值一致。
- 最终 clippy、测试政策和 whitespace 检查通过。
- `serve-regression-known-failure-excluded.log` 运行 55 条：
  51 通过、4 条显式资源/fixture gate，仅过滤已复现且等待生产修改授权的
  并发填充 case；不冒充完整套件绿灯。

本页未运行 Analyze 成功的真实模型链，也未验证 library description 模型或 RAW sidecar
匹配；已有底层 library 测试不能替代这些尚未执行的 HTTP 成功分支。

## 2026-11-23 当前复核

- `http_analyze` 当前包含两条产品 case 和一条显式 fixture worker；两条产品 case
  通过真实 `autoshade serve` 基线，worker 不计作产品 case。
- 删除 `harness::verdict_from` 的 `accept` 分支时，成功 case 在响应 verdict
  `revise != accept` 处失败；该定向 mutation 为 1 caught。
- 将 `store::develop_revision` 替换为 `None` 时，成功 case 捕获 revision 丢失；
  该定向 mutation 为 1 caught。
- 反转 `harness::run_inner` 的子进程成功判断时，提交前失败 case 在错误不再说明
  `harness failed` 处失败；该定向 mutation 为 1 caught。
- 以上仍未分别破坏响应 recipe、`saved` 以及失败时旧 recipe 的保留，因此表中两项
  继续保持未完成。fixture 目前也未构造“已写 develop 后非零退出”，不能把提交前
  失败外推为部分失败原子性。
- 随后增加的部分提交 fault fixture 先写 exposure 1.5，再以状态 10 退出。HTTP
  返回 500，但保存从 exposure 0.5 及旧 revision 变成 1.5 及新 revision；新增 case
  确定性红，证明当前 API 会“报告失败但已替换保存”。修复需要生产协议或回滚变更，
  用户随后决定暂时跳过 Harness；case 以明确缺陷原因和单条启用命令进入临时显式
  gate，仍可确定性复现。
