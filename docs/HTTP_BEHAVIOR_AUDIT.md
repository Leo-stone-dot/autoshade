# HTTP 行为保护复核

日期：2026-11-23。范围：本轮检查和补测，不代表全仓行为清单。

## 复核结论

- 当前生产源码相对本轮开始不作修改。开始时已有的 `src/bin/gui/main.rs`
  改动仅为 `#[cfg(test)] mod tests;`，保留。
- 本轮重新运行默认 release + GUI 套件通过；政策与架构扫描及其 self-test
  通过。它们只能说明本次执行的检查通过，不能证明所有测试符合原则或没有漏保。
- `tests/serve_behavior.rs` 开始时只有导出、下载、预览三条成功路径，
  且均需要显式运行。`store_behavior` 的锁测试不能替代 HTTP 的 revision /
  generation 校验：HTTP 请求可能在进入存储锁之前就选错照片或接受旧版本。
- 旧清单使用“搜索函数名没有直接调用”作为未保护证据不成立；间接调用也能保护行为。
  本轮逐项比较真实入口、前提和断言，而不以函数名出现次数判断。
- mutation 的 `--list` 不是执行证据。存活的等价 mutant 不等于产品漏洞；
  编译失败和超时也不计作测试发现故障。每个反例必须有通过的基线、
  可编译的行为破坏，以及对应断言的失败。
- 增量 mutation 只检查选中的范围。当前工作区含 GUI 入口的测试模块接线，
  wrapper 因此走“生产文件变化”分支，不要求为新增 HTTP 测试指定被保护文件。
  本轮另行定向验证 `src/serve.rs`，不将 worktree gate 的范围冒充 HTTP 证据。
- 84 个已勾选条目不是全仓完整性证明。未执行“移除必要保护后完整门禁仍接受”
  的端到端反例，因此也不声称已经证明整个完整性机制无效。

## 本轮行为清单

所有入口均为真实 `autoshade serve` 的 HTTP API，下面的函数名只定位实现，
不作为直接测试对象。此前三条 HTTP 测试均未断言以下行为。
勾选表示该项行为测试与定向反例已实际执行，不表示整个仓库门禁通过。

| 状态 | 行为与实现定位 | 前提及断言计划 | 故障反例 |
| --- | --- | --- | --- |
| [x] | 保存后重新打开同一编辑；`api_xmp` / `api_recipe` | 合成 PNG，经 HTTP 保存非中性曝光，再读回值和 revision；源照片字节不变 | 保存时交付默认 recipe；写坏源照片 |
| [x] | 缺少或错误会话令牌不能改写；`handle` | 先合法保存，再发无令牌/错误令牌的保存；403，原编辑不变 | 跳过 POST 令牌拒绝 |
| [x] | 外站请求不能修改本地照片；`foreign_origin` | 有合法令牌，但 Host / Origin / Fetch Metadata 来自外站；403，原编辑不变 | 分别删除 Origin、Host、Fetch Metadata、port 拒绝 |
| [x] | 图片读取需要当前会话；`handle` | 缺少/错误图片 token 被拒绝，当前 token 可读取正确照片 | 跳过图片令牌拒绝 |
| [x] | 切换目录后旧照片 id 不得写入新照片；`stale_generation` / `at_checked` | 两个目录各有 id=0；切换后提交旧 gen，409，新照片未保存 | 跳过 generation 拒绝 |
| [x] | 正在上传的旧请求也不能写错照片；`AppState::at_checked` | HTTP 100-continue 确认请求开始读正文，再切换目录，最后提交正文；409，新照片未保存 | 删除读完正文后的 generation 复核 |
| [x] | 过期编辑不能覆盖新版本；`precondition_failed` | 两次合法保存后用旧 ETag 保存；412，返回当前 ETag，内容不变 | 跳过 If-Match 校验 |
| [x] | 过期重置不能删除新版本；`api_xmp` | 旧 ETag 提交中性 recipe；412，新编辑仍可读 | 让 clear 绕过 If-Match |
| [x] | 同一 revision 的并发写入至多成功一次；`api_xmp` | 两请求由 barrier 同时提交；一个 200、一个 412，重新打开为成功者内容 | 接受不匹配的 revision |
| [x] | 非本次会话签发的 master 不能安装；`session_master` | 传入真实存在但未签发的图像路径；400，已保存编辑不变 | 接受任意可读 master |
| [x] | 非法 JSON 是客户错误且不改写；`read_json` / `handle` | 已有编辑后发送坏 JSON / 非 UTF-8；400，原编辑不变 | 将客户错误转为 500 |
| [x] | 空上传不能留下图库项；`api_upload` | 上传空 body；400，图库不变 | 接受空上传 |
| [x] | 同名并发上传保留两张照片；`api_upload` | 两张不同颜色合成 PNG 同名上传；两个不同 id，读回各自像素 | 用截断写代替独占创建 |
| [x] | 重复导入不重复添加；`api_import` | 导入新照片再重复导入；第一次 added=1，第二次 added=0，图库只增一项 | 取消导入去重 |
| [x] | 导入失败不损坏现有图库；`api_import` | 成功导入后提交不存在路径；400，原图库和已导入照片仍可用 | 报错前清空图库 |
| [x] | RAW 保存中的 XMP 合并损失不能静默；`api_xmp` | 合成 DNG 配不可合并的 sidecar；编辑仍可重开，生成的 XMP 可解析，响应披露损失，原 sidecar 未改写 | 丢弃 merge note；写坏原 sidecar |

## 执行与边界

运行：`cargo test --locked --release --features gui --test serve_behavior`。
这 19 条测试进入默认套件，不访问付费服务、模型或外部网络。
每个子进程独立 cwd / 数据目录 / 环境 / 动态端口；HTTP 和启动等待均有超时。
并发使用 barrier 或协议应答，不用 sleep 猜测时序。
Agent / release battery 和 build / release CI 现有的 `cargo test` 会实际执行它们。
本轮曾添加独立 HTTP 步骤，但它无法让默认 mutation 下的 ignored case 执行；
因此最终撤掉这些重复步骤，将自包含的本地 HTTP 用例纳入默认套件。
CI 在远端尚未执行，不冒充 Linux 验证。

不把本批边界误认为整个 HTTP 服务已完整保护：大体积 body 的资源预算、
完整 retouch / heal / stack 工作流、真实浏览器交互、外部模型失败仍需另行审计。
本批也不替代 fit 慢套件或其他平台验证。覆盖率仅为诊断资料，不决定勾选。

## 本轮执行证据

`tests/serve_behavior.rs` 共 19 条：新增 16 条，原有 3 条保留。
原有导出、下载测试的 JPEG 魔数检查改为真实解码并核对完整尺寸；
相应截断反例也已执行。

本机临时证据目录：`/private/tmp/autoshade-http-audit.8mnh3v`。
它不是测试依赖，也不是新增的函数登记表。目录内：

- `run_mutations.py`：复制当前源码和测试至独立目录，在副本中逐个注入故障。
- `baseline.log` / `restored.log`：同一套 19 条 HTTP 用例在注入前和恢复后全部通过。
- `results.json`：23 个反例的 case、退出码和结果；每个均编译成功，
  实际运行唯一指定 case 并产生断言失败，无 timeout / unviable 计作 caught。
- 每个同名 `.patch` / `.log`：具体改坏的代码和失败位置。
- `production-before.json` / `production-after.json`：269 个生产 Rust 文件校验值相同。
- `http-default-mutation-gate.log`：当前 wrapper 在不传 `--ignored` 的情况下
  实际捕获 `image_token_matches -> true`，失败为错误令牌得到 200 而非 403。
  `image-token.diff` 仅用于限定这次实验的 mutation 行，不是工作区改动；
  列表确认只有一个候选后再执行。该定向通过不替代 GUI 范围的失败结果。

| 反例文件名 | 失败证据 |
| --- | --- |
| `save-default` | 保存后重开返回 404，而不是保存的编辑 |
| `overwrite-source` | 原照片字节不一致 |
| `post-token`、`foreign-origin`、`foreign-host`、`cross-site`、`foreign-port`、`image-token` | 应拒绝的请求返回 200，而非 403 |
| `old-gallery`、`late-gallery` | 旧照片请求返回 200，而非 409 |
| `stale-save`、`stale-clear` | 旧 revision 写入/删除返回 200，而非 412 |
| `concurrent-revision` | 两个写入都返回 200，而非只接受一个 |
| `unissued-master` | 非本次签发的 master 被接受 |
| `client-error` | 非法 body 返回 500，而非 400 |
| `empty-upload` | 空上传返回成功 |
| `truncate-upload` | 两张不同照片被分配为同一 id |
| `import-duplicates` | 第二次导入仍新增照片 |
| `import-erases-gallery` | 失败请求清空了原有图库 |
| `merge-loss-silence` | 保存响应不再披露合并损失 |
| `overwrite-sidecar` | 原 Lightroom sidecar 字节被替换 |
| `truncated-export`、`truncated-download` | 图像无法完整解码 |

回归结果：

- 默认 release + GUI 套件及 19 条 HTTP 用例通过；最终版本将 HTTP 纳入默认执行。
- `scripts/fit_battery.sh` 的 2 + 11 条显式慢用例通过；不是仅凭 ignore 推断未执行。
- clippy、测试政策、架构、文本、文档漂移与文档声明检查通过；
  文档声明中外部 XMP census 因无语料未执行。
- 系统 Python 缺 `fontTools`；临时 venv 安装该依赖后，字体子集检查通过。
- `scripts/release_battery.sh` 的中间版本完整执行通过，记录为 `release-battery.txt`；
  最终默认 HTTP 版本也完整通过，记录为 `release-battery-final.txt`。
  外部 XMP census 仍明确 skip。
- 完整 Agent 门禁仍为失败：worktree mutation 回退至 GUI 入口文件。
  第一次是 15 missed、3 unviable；接入 HTTP gate 后再次完整执行，
  是 14 missed、1 caught、3 unviable，日志为 `agent-quality.log`。
  下节说明为何这个 caught 不能当作有效保护。两次完整结果分别保存在
  `worktree-mutations-first` / `worktree-mutations-second`。
  wrapper 的两条结果文案已纠正，不改变退出码或放松判定。

GUI 候选的初步定位（不冒充已补保护）：

- 将 GUI 的 `main` 改为直接 `Ok(())` 仍被记录为 missed；
  它移除了真实启动事件循环，说明当前自动测试没有验收“启动后呈现可操作窗口”。
  这不是要求某个私有函数必须被调用。
- `message_box` 位于 `#[cfg(windows)]` 中。本次 macOS 运行不能验证它，
  不能把该候选算作 macOS 的产品行为缺口或当作全平台等价变化。
- `adopt_prefs_between`、`adopt_pre_rename_prefs` 和 `ADOPT_PRE_RENAME_PREFS`
  对应启动时保留偏好、迁移失败回退与平台 opt-out，需经真实启动入口验收；
  不恢复直接调用私有 helper 的旧测试。
- panic 报告、图标、窗口约束与 `persistence_path` 对应其各自用户可观察结果；
  本批 HTTP 测试不证明这些结果，源码匹配也不能代替。

## 复核中发现的无关失败

第二次门禁把 `app_icon -> Default::default()` 算作 caught，但其失败日志
是 `tests/process_behavior.rs` 的“父进程退出码必须非零”，不是图标断言。
该 GUI mutation 不改变进程测试链接的库代码，不能拿此失败证明图标已受保护。

修正只在测试：

- 保留原 case，不限制父 shell 的退出码；契约是取消后整棵进程树结束。
- 清空 fixture 子进程环境，用保持打开的输入管道维持它，而不是让它在 30 秒后自行退出。
- 取消前确认子进程存在，取消后在期限内检查父、子进程都结束。
- `verify_process.py` 实测 20 次独立基线均通过；“只杀父进程”的反例
  在子进程存活断言失败，“不取消”的反例在父进程存活断言失败；恢复后通过。
  证据为 `process-*.log`，故障仍只注入副本。

图标 mutant 的独立对照复跑记录在 `icon-*.log`：修正进程断言后，
原版、图标 mutant、恢复版的默认套件均通过，图标仍然没有被这些测试保护。
与 HTTP 的 23 个定向反例分开报告，不将进程测试的修正当成图标保护。

## 覆盖率采集限制

本次单独启用 HTTP gate，尝试通过 LLVM 的 `%c` 连续 profile 采集服务子进程。
19 条 HTTP 测试通过，但工具报告 `Counters section not page-aligned`，
生成的 `http-coverage.json` 仍将 `src/serve.rs` 的执行计数记为零。
该报告不能用于评价本批保护，更不能据此把真实已执行的行为再次判为漏测。
未报告虚假的覆盖率提升，未用该数字决定测试取舍。
后续曾在当前 macOS 工具链用单个 export case 验证三段页对齐能采到服务子进程，
见 `MASTER_WORKFLOW_AUDIT.md`。本轮扩大到多测试目标时发生 SIGSEGV，因此该参数
不是通用采集方案，也没有生成可用的全仓报告；不将它当作测试保护提升。

## 尚未完成

后续像素工作流、请求上限及 GUI 环境核验记录在 `MASTER_WORKFLOW_AUDIT.md`。

- [ ] GUI 入口候选逐项区分：非等价漏保、等价变化、平台未执行及无关测试失败。
  这是独立待审范围，不以本批 HTTP 绿灯销账。
- [ ] 建立不使普通测试二进制崩溃、又能采集服务子进程的覆盖率诊断方式。
  当前只证明单个 export 探针可用；本轮全套在 `judge_behavior`、补跑在
  `cli_behavior` 均 SIGSEGV，不能外推。
- [ ] 资源预算及 retouch / heal / stack 等未纳入本批的 HTTP 行为另行审计。
