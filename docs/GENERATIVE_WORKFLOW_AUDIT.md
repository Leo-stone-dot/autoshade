# 生成式填充行为验收

日期：2026-11-23。延续 `MASTER_WORKFLOW_AUDIT.md`；不替代未完成的原生 GUI 验收。

## 现有保护与缺口

`tests/generative_behavior.rs` 的两条用例验证独立 reimagine 成功/失败；
现有 HTTP heal 用例不经过生成式填充的模型等待、合成和 master 签发。
本轮通过真实 `autoshade serve` 验证下表，不直接调用私有 helper。
外部模型只由隔离的 Comfy CLI fixture 替代，不安装模型、不请求真实服务。

| 状态 | 行为及实现定位 | 前提与公开断言 | 故障反例 |
| --- | --- | --- | --- |
| [x] | `/api/retouch` / `generative::retouch` 的区域合成 | 模型故意改动涂选区和远处区域；最终 master 与响应预览呈现涂选区变化，远处仍为源图，原文件不变 | 不合成；把模型结果替换到未选区域；改写源图；响应预览仍用旧源图 |
| [x] | 送往外部模型的图像、蒙版和指令 | 从 fixture 收到的 workflow 和上传文件验证源图、白色选中区、黑色保留区以及用户指令 | 发送空白图像；反转模型蒙版；丢掉用户指令 |
| [x] | 连续填充保留上一结果；`api_retouch` | 第二次携带第一次签发的 master，最终同时保留两处变化 | 第二次从原图重新开始 |
| [x] | 已完成填充能够保存和重开 | 中性 recipe 保存 fill master，服务重启后预览仍有填充 | 不签发可用于后续操作的 master |
| [x] | 生成像素 provenance 跨填充、保存、重开保留 | 用公开 Store API 建立生成像素前提；曲线须先能改变普通照片，填充后不得重复应用已烘焙的基准曲线 | 签发或保存时丢掉 generated 标记 |
| [x] | 损坏模型输出不破坏此前结果 | 第一次填充成功，第二次返回损坏图像；明确失败，旧 master 不变，合法重试可继续 | 失败清理错误删除此前 master；错误报告成功 |
| [x] | 模型失败不能因残留下载图像而被当成成功 | CLI run 失败但 download 留下完整图片；API 仍拒绝发布 | 忽略模型进程失败状态 |
| [x] | 模型等待不阻塞导出；`edit_onto` | fixture 确认模型请求正在等待，照片导出须在放行模型前完成 | 将 full-resolution slot 持有到模型返回之后 |
| [ ] | 同时填充的交付相互独立 | 两个 HTTP 请求同时发出，允许内部排队；完成后两份交付各自保留其涂选区结果 | 两次操作复用同一交付文件 |
| [x] | 旧 reimagine 成功用例的完整性交付断言 | 保留 PNG 格式检查，补完整解码与模型回复像素一致性 | 交付仅含 PNG 文件头 |

## 隔离与运行

- CLI fixture 只实现其公开的 env/upload/run/download 协议，未知命令明确失败。
- fixture 的 PNG、JSON 和管道均位于每例独立目录，fixture 不生成测试断言结果。
- 模型等待由 FIFO 通知和独立放行事件驱动；所有等待有截止时间。
- 需要多进程的生成像素前提由测试 worker 调用公开 Store API 建立，避免改写
  并行测试进程的环境变量。worker 不是新增产品 case，缺少前提时应明确失败。
- 模型 fixture 使用 POSIX shell，因此本批对应 Unix 平台；其它平台不以源码存在代替运行。
- 测试服务退出时清理自己的进程组，避免模型 fixture 留在后台。

运行：`cargo test --locked --release --features gui --test serve_behavior http_fill::`。

## 执行证据

本机证据目录：`/private/tmp/autoshade-fill-audit.KzBH91`。

### 未修改生产代码即复现的缺陷

并发用例不能标成通过，也不是普通 flaky。用户随后决定暂时跳过 Comfy，因此它以
明确缺陷原因和单条启用命令进入临时显式 gate；测试与红灯证据保留：

- 单独运行曾通过；模块并行重复运行在 `repeat-01.log` 再次失败。
- API 返回 500，正文为 `retouch failed: comfy upload`。
- 外部 CLI fixture 的诊断为上传源不存在：
  `custom-mask-89517-1795413362896.png: No such file or directory`。
- `comfy::custom_workflow` 用同一进程 ID 与毫秒时间戳构造 input/mask 路径，
  上传完成后删除 mask。两个请求命中同一毫秒时会共享该路径，一个请求的清理
  能删除另一个请求仍需上传的文件。`run_workflow_json` 的 workflow/download
  路径使用同类命名，也需要同一轮修复覆盖。
- 已请求生产修改授权；建议复用现有唯一序号，只改变临时文件身份，
  不改变传给模型的 seed、公开 API 或生产资源上限。尚未修改生产源码。

在等待授权期间新增的 HTTP 设置行为验收及证据见
`SETTINGS_BEHAVIOR_AUDIT.md`；它不掩盖本页的失败项。

### 对此前测试的反向验证

`truncated-reimagine-before.log`：把最终文件截为 PNG 的 8 字节文件头，
旧用例仍通过。增加完整解码和模型图像的 RGBA 内容比对后，
`truncated-reimagine-after.log` 在 `UnexpectedEof` 处失败。
这不依赖覆盖率或新增 case 数量。

生成像素的第一版用例也未能杀死丢标记的反例：构造的曲线只有端点，
被端点归一化规则变成了中性曲线。修正为含非中性中间控制点的曲线，
并先用普通照片证明它会改变画面；随后分别验证填充和保存后的生成像素
不再被校准。两个丢标记反例均使图像结果断言失败。

### 定向验证范围

`run_mutations.py` 仅对已通过的 9 条用例逐条建立基线，不包含上述
尚未修复的并发用例。`mutation-results.json` 记录 16 个反例，全部在
相应行为断言处失败；恢复副本后这 9 条基线再次通过。
每个反例都有同名 `.patch` 和 `.log`。这不是全套测试通过的声明。

- `no-fill`、`overwrite-unselected`、`overwrite-source`、`stale-preview`
  分别暴露区域没有改变、远处被替换、原文件被修改、预览仍为原片。
- `blank-model-input`、`inverted-provider-mask`、`lost-user-instruction`
  在实际发送给外部 provider 的内容处失败；fixture 并未替产品决定合成结果。
- `fill-forgets-master` 丢失第一处填充；`unusable-fill-capability` 导致保存返回 400。
- `lost-issued-origin` / `lost-saved-origin` 分别使会话预览、重开结果重复应用曲线。
- `failed-fill-deletes-previous` 删除了此前交付；
  `failed-fill-reports-success` / `ignored-model-exit` 错误返回 200。
- `model-wait-blocks-export` 在已观察到外部模型等待的前提下，
  导出未能在模型放行前完成，命中用例的有界等待断言；
  不是 cargo 超时被误算为 caught。
- `truncated-reimagine` 在交付解码时失败。

此前 HTTP 回归单独执行为 29 条通过、3 条资源用例未在该命令执行
（`existing-http-regression.log`）；reimagine 两条用例通过。
生产源码校验保存在 `production-before.json` / `production-after.json`，
269 个生产 Rust 文件保持一致。

后续设置 API 的 5 条行为用例和 10 个故障反例见
`SETTINGS_BEHAVIOR_AUDIT.md`；全套 HTTP 回归仍明确排除本页已复现、
尚未获授权修复的并发填充 case。
