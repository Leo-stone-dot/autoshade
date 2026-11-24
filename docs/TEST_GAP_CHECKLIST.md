# 测试缺口待完成清单

状态：2026-11-23 本轮有效边界已完成；A2-A5 按用户决定暂缓

历史批次记录：84 项已勾选。该记录不是当前全仓待办总数，也不是本轮重新验证过的完成结论。2026-11-23 的复核、纠正和新增待办见 `docs/HTTP_BEHAVIOR_AUDIT.md`。

验收原则与指标定义见 `docs/TESTING_PRINCIPLES.md`；自动 mutation 门禁见 `scripts/mutation_gate.py` 和 `scripts/agent_quality_gate.sh`。

本清单只记录已审计的缺口，不是函数全集。它不是自动门禁的分母；自动门禁只看本次改动产生的 mutation 是否被测试杀死。

条目里的函数名如果指向私有 helper，只作为定位信息，不是直接测试目标。验收必须从公开入口、可见 UI、文件结果或错误契约进入。

本清单记录当前行为测试没有保护的函数。勾选条件不是“写了测试”，而是同时满足：

1. 测试断言公开行为，不读取 Rust 源码，不绑定私有字段或 helper。
2. 测试有真实 premise，失败时对应产品行为确实坏了。
3. 新增不变量有一个能杀死它的 mutation。
4. 默认快速、确定、可并行；外部或慢路径显式 gate，但仍属于测试范围，不能因为慢而跳过行为。速度只决定 gate，不决定入选。
5. 全量测试、clippy 和对应文档/发布门通过。

## 2026-11-23 当前复核边界

这一轮不把历史勾选重新解释为“全仓完整”。下面保留八项复核记录；用户决定暂时
跳过 Comfy 与 Harness，因此 A2-A5 不参与本轮完成判定，但测试和红灯证据保留，
不删除、不 `ignore`。本轮有效边界为 A1、A6、A7、A8。完成后仍须列出未审计的
公开工作流，不能据此宣称所有产品行为都已保护。实现函数只用于定位，测试仍从
HTTP、CLI、文件结果或可见 GUI 进入。

### [x] A1 当前证据与门禁结论可复现

- **目标**：默认行为套件、显式慢 gate、测试政策、Clippy 和 mutation
  工具分别报告自己的实际执行范围；临时目录中的历史日志不作为当前绿灯。
- **现有缺口**：审计文档仍混有旧计数、`/private/tmp` 中的一次性 Python runner
  结果和“生产文件未变”等历史结论。当前 `serve_behavior` 实际列出 60 条，其中
  7 条 ignore；`cargo-mutants 27.1.0` 位于 `/private/tmp/cargo-mutants/bin`，未加入
  默认 `PATH` 时 mutation wrapper 不能执行。
- **故障风险**：门禁没有运行或运行了错误范围，却被误报为行为已保护。
- **验证**：保存本轮命令、测试数、ignore 原因、失败项及生产 diff；门禁失败保持
  失败，不用文档解释替代。
- **本轮证据**：当前扫描到 25 个 Rust 测试文件、383 个 `#[test]` 入口和 21 个
  `#[ignore]` 标记；这些是执行范围清单，不是质量分数。按用户决定暂缓 A4/A5 后，
  当前不带手工过滤的 release + GUI 回归为 360 passed、21 ignored、0 failed。
  A5 四次并行复跑为 2 passed / 2 failed，A4 单独运行确定性失败；两者没有被改写成
  通过。Clippy、测试政策、文本、文档漂移和文档声明通过；
  文档声明的外部 XMP census 因未配置语料明确 skip。字体检查因系统 Python 缺少
  `fontTools` 未运行。
- **门禁范围纠正**：原 wrapper 在两行 `cfg(test)` 接线没有 mutant 时，退化为
  GUI `main.rs` 全文件 18 个无关候选。现已改为诚实失败；只有显式设置
  `AUTOSHADE_MUTATION_FILES` 才改测所声明的行为文件。无映射时实测退出 1，指定
  `src/decode.rs` 时只进入该文件范围。统一 `agent_quality_gate.sh` 也已实际运行：
  该次 A5 碰巧通过，常规测试 / clippy / 政策与文档 gate 通过，最终在缺少显式
  mutation 行为映射处退出 1。统一门禁仍不宣称为绿；本轮 scoped 回归命令逐字记录
  两个 filtered case。

### [暂缓] A2 Harness Analyze 成功结果与已提交 develop 一致

- **公开入口 / 定位**：真实 `POST /api/analyze`；`serve::api_analyze`、
  `harness::run`。
- **目标行为**：响应中的 recipe、Accept verdict、`saved` 和 revision 与随后
  `GET /api/recipe` 及重启后读取一致，源照片不变。
- **现有缺口**：`http_analyze::a_successful_http_harness_analysis_returns_and_reopens_its_committed_recipe`
  当前基线通过，但此前没有可复核的定向 mutation 执行。
- **故障风险**：服务器返回模型候选或虚假保存状态，而磁盘上是另一份 develop。
- **验证**：只运行该 HTTP case；用 `cargo-mutants` 或最小可编译故障反例分别破坏
  响应 recipe / verdict / saved / revision 的一致性，确认对应公开断言失败。

### [暂缓] A3 Harness 在提交前失败不改变已有保存

- **公开入口 / 定位**：真实 `POST /api/analyze`；`harness::run_inner` 的子进程失败
  分支。
- **目标行为**：harness 在提交前非零退出时返回明确错误，已有 recipe、revision
  和源照片保持不变。
- **现有保护**：`http_analyze::a_harness_failure_before_commit_leaves_the_previous_save_available`
  的基线通过；名称已收窄到 fixture 实际构造的写入前失败，不能外推到外部 harness
  已部分写入后的恢复。
- **故障风险**：分析失败误删或覆盖已有编辑，或者错误报告成功。
- **验证**：把 case 名和文档收窄到真实 premise；定向破坏失败状态处理时，该 case
  必须在 HTTP 状态、错误或旧保存断言处失败。

### [暂缓] A4 Harness 部分提交后失败的原子性

- **公开入口 / 定位**：真实 `POST /api/analyze`；外部 harness 当前直接写 develop，
  `harness::run_inner` 在子进程成功后才读取结果。
- **目标行为**：若 harness 已写入部分结果后非零退出，旧 develop 不能被静默替换，
  或 API 必须准确披露已经提交的新结果；不能同时返回失败并留下不明状态。
- **现有缺口**：当前 fixture 在任何写入前退出，未覆盖部分失败。
- **故障风险**：用户看到“分析失败”，实际旧编辑已被替换，重试后无法判断状态。
- **验证**：先用只改 fixture 的故障场景确认当前外部契约；若产品行为不满足，保留
  明确红灯并在修改 `src/harness.rs` 或协议前请求授权。
- **执行证据**：fixture 先把 exposure 1.5 写入目标 develop，再以状态 10 退出。
  真实 HTTP 返回 500，但随后 `GET /api/recipe` 从原 exposure 0.5 / 原 revision
  变为 1.5 / 新 revision。case 在“失败响应必须保留旧保存”处确定性失败。当前实现
  需要生产协议或回滚修改。用户决定暂时跳过 Harness；本项不参与本轮完成判定，
  以明确缺陷原因和 `--ignored --exact` 启用命令保留。

### [暂缓] A5 同时生成式填充的临时资源互不冲突

- **公开入口 / 定位**：两个并发 `POST /api/retouch`；`comfy::custom_workflow` 和
  `run_workflow_json`。
- **目标行为**：两次填充可以排队，但各自的上传、workflow、下载和交付不能覆盖或
  删除另一请求的资源。
- **现有缺口**：公开行为 case 已存在，但本轮四次并行复跑为 2 通过、2 失败。失败
  均为 `pid + 毫秒` 同名 mask 被另一请求删除；它是有效产品红灯，但不是稳定绿门禁。
- **故障风险**：并发填充随机返回 500，或交付另一请求的像素。
- **验证**：生产修复需先获授权；修复后重复并发 case，并用强制复用临时身份的定向
  反例证明测试会失败。不得重试到绿或放宽断言。
- **暂缓决定**：用户决定暂时跳过 Comfy；本项不参与本轮完成判定，测试与失败证据
  以明确缺陷原因和 `--ignored --exact` 启用命令保留。

### [x] A6 删除源码正则架构扫描

- **目标**：架构约束只由 rustc、类型、可见性或 Clippy 执行；产品结果另有行为测试。
- **结论**：自建架构脚本只匹配特定源码拼写、调用顺序和文件位置，不能覆盖等价 Rust
  写法，已删除。只有 GUI logic headless 规则由 `clippy.toml` 与 module-scoped deny
  继续执行。
- **后续**：原脚本声称覆盖的 paint refresh、RAW 上限、slot 生命周期、Store bounded
  reads、base look parity 等项目全部重新打开，等待公开行为或不可绕过的类型设计。
  不用 lint 勾销。
- **本轮行为证据**：`tests/decode_behavior.rs` 的
  `an_oversized_declared_raw_plane_is_refused_before_the_decoder_can_allocate_it`
  通过公开 `decode_raw` 拒绝一个声明为 50001×1 CFA plane 的最小 TIFF。将
  `guard_raw_plane_extent` 整体替换为 `Ok(())` 后，该 case 收到后续“未知 RAW”
  错误并失败。最终用只标记该函数入口行的 diff 重跑，cargo-mutants 只列出一个候选，
  基线通过并得到 `1 caught`。
- **未验证边界**：paint-buffer 修改后的真实蒙版刷新、4 GiB RAW develop ceiling、
  原生 GUI 启动 / 偏好，以及 full-resolution slot 的全部资源结果仍未得到本轮执行
  证据。它们没有被十二条正则规则勾销，明确留给后续具备执行环境的行为审计。

### [x] A7 回归、慢 gate 与诊断覆盖率

- **目标**：A2-A6 的有效改动完成后运行相关默认回归、显式资源 / fit gate、clippy
  和政策检查；生产 Rust 除既有 `#[cfg(test)] mod tests;` 接线外不变。
- **现有缺口**：完整默认套件本轮虽曾通过，但 A5 可重复红；远端 Linux、原生 GUI
  启动 / 偏好、Windows 分支、真实模型和真实大 RAW 未验证。
- **故障风险**：窄 case 通过被外推为跨平台、慢路径或完整产品已安全。
- **验证**：覆盖率只在行为审计后生成一次前后诊断，列出未触达区域，不据数字选择
  case 或判断完成；最终报告所有未执行 gate 和已知红灯。
- **本轮证据**：fit 慢 gate 为 2 + 11 passed；HTTP 资源 gate 为 2 + 1 passed。
  覆盖率全套在 `judge_behavior`、补跑在 `cli_behavior` 的插桩二进制 SIGSEGV，
  没有生成可用报告。该失败只记录诊断工具限制，不改变测试选择，也不把旧百分比沿用
  为当前结果。当前默认回归为 360 passed、21 ignored、0 failed；A4/A5 以带原因
  和启用命令的临时显式 gate 保留，A4 已用 `--ignored --exact` 再次确定性复现。
  clippy 与相关政策 / 文档 gate 通过。生产 Rust 除既有
  `#[cfg(test)] mod tests;` 两行接线外无 diff。

### [x] A8 CLI 的本地文件交付边界

- **公开入口 / 定位**：真实 `autoshade decode` 和 `autoshade apply` 子进程；
  `main::decode_cmd`、`main::apply_cmd`。
- **目标行为**：decode 的显式目标是完整可解码 preview；apply 读取公开 recipe、
  交付实际 develop 且不改源照片；任何命令都不能把输出覆盖到输入照片。
- **现有缺口**：当前进程级测试只启动 `serve` 和 `mcp`。底层 decode、render 和
  preflight case 不能证明 clap 路由、参数和 CLI 文件编排没有接错。
- **故障风险**：命令返回成功却无交付、忽略 recipe，或把源照片当输出覆盖。
- **验证**：合成 PNG 和序列化 recipe，运行真实 release CLI；分别将 `decode_cmd` /
  `apply_cmd` 替换为提前成功、将只读输出 guard 替换为成功，指定 case 必须失败。
- **执行证据**：`tests/cli_behavior.rs` 三条基线通过。三个 unified diff 只选择
  `decode_cmd -> Ok(())`、`apply_cmd -> Ok(())` 和
  `guard_readonly -> Ok(())`，每次 cargo-mutants 均列出一个候选并得到
  `1 caught`。其余 CLI 命令未纳入本项，仍属于后续公开工作流审计范围。

## 历史证据基线

以下统计属于此前批次，不代表当前测试总数或本轮执行结果；新增与复核结果见
`docs/HTTP_BEHAVIOR_AUDIT.md`。

- 当前行为测试：`src/bin/gui/tests.rs`、`tests/product_behavior.rs`、`tests/store_behavior.rs`、`tests/domain_behavior.rs`、`tests/properties.rs`、`tests/decode_behavior.rs`、`tests/denoise_behavior.rs`、`tests/process_behavior.rs`、`tests/fit_behavior.rs`、`tests/fit_workflow.rs`、`tests/library_behavior.rs`、`tests/serve_behavior.rs`、`tests/mcp_behavior.rs`、`tests/judge_behavior.rs`、`tests/generative_behavior.rs`，共 322 个（74 GUI + 78 product + 34 store + 81 domain + 9 properties + 3 decode + 5 denoise + 1 process + 15 fit + 11 fit workflow + 2 library + 3 serve + 2 mcp + 2 judge + 2 generative）；其中 13 个 fit 用例和 3 个 HTTP 用例是显式慢 gate，1 个 lock worker 由父测试显式启动。
- 覆盖率只作为诊断，不作为完成指标、考核指标或测试质量结论；报告在 `/private/tmp/autoshade-coverage-current.txt`，用 `cargo llvm-cov --locked --features gui --all-targets --summary-only` 生成。
- `tests/support/mod.rs` 和 `src/bin/gui/tests/support.rs` 提供按进程和 case 隔离的临时目录与环境变量沙箱；测试文件不各自复制清理逻辑。
- 历史选材曾搜索函数名以寻找直接调用；这只能定位候选，不能证明没有行为保护。间接调用也可能提供有效断言，需核对入口、前提和结果。
- 外部系统测试允许 stub 或 gate，但不得 mock 被测函数本身；测试名或注释必须说明 gate 原因。

## Mutation gate baseline

### [x] `decode_any_turned` 的 baked 转向
- 位置：`src/decode.rs:825`。
- Case：`decoding_a_baked_image_with_a_quarter_turn_swaps_pixels_and_meta_dimensions`
- Premise：4×2 合成 PNG，请求 `quarter_turns = 1`。
- Act：调用公开入口 `decode_any_turned`。
- Assert：preview 和 Meta 尺寸都变为 2×4。
- Mutation：删除 baked 分支的 Meta 尺寸更新时，测试必须红；已用 release 实测。
- Gate：fast。

### [x] `decode_raw_turned` 的无内嵌预览回退必须保留转向
- 公开入口：`autoshade::decode::decode_raw_turned`，回退路径进入私有 `neutral_rendition`。
- Case：`a_raw_without_an_embedded_preview_honors_the_requested_quarter_turn`（`tests/decode_behavior.rs`）
- Premise：用 rawler DNG writer 合成 16×8、无 embedded preview/thumbnail 的 DNG，请求 `quarter_turns = 1`。
- Act：调用公开入口 `decode_raw_turned`。
- Assert：交付 preview 和 Meta 尺寸都变成 8×16；真实走 `neutral_rendition` 回退。
- Mutation：把 `neutral_rendition(path, quarter_turns)` 改成 `neutral_rendition(path, 0)` 时，尺寸断言变红；已用 release 实测。
- Gate：fast；debug 实测 0.23 s，进入默认套件。

## 本轮新增保护

- `tests/store_behavior.rs`：孤儿 recipe/pixel/variant 备份恢复及 live-wins、程序化备份版本与 raster 冻结失败、单代 recipe/pixels/variants 提交及 oversized 成员失败、提交点之后发布失败的下次恢复、analyzed RAW 同时发布 recipe/XMP、baked analyzed 清理 projection、XMP 发布失败保留旧 projection、analyzed 提交点后冲突恢复、未解析 variants 拒绝覆盖、malformed/future/unknown-kind/escape variants 拒绝、variant kind 读改写保留未知字段、saved quarter-turn 归一化、recipe revision 随内容变化、develop snapshot 同时读取 recipe/XMP/pixel source 与空快照、导出 XMP 不覆盖 Lightroom 已有 sidecar。
- `tests/product_behavior.rs`：目录输出目标在渲染前被拒绝、已有照片输出目标不被替换、新输出目标创建父目录、只读目录与 symlink 输出被拒绝、RAW 扩展名大小写匹配且排除 baked 文件、evidence luma bin clamp 与灰色像素不进入 hue band、结构化图像跨多个 luma evidence range 而平坦图像只落在一个 range、无 target presence 时 evidence population 为零、缺失 segmentation sidecar 在创建 artifact 前失败、缺失 harness key 在解码前拒绝、baked 旧 coord era 只升级不 reframe、裁剪 aspect 被 same-frame 判据拒绝、过小结构 core 返回 abstention、XMP projection 写出后可重新解析。
- `tests/product_behavior.rs`（本轮追加）：真实 deferred harness 结果进入 `produce_recipe`；带 EXIF 转向的旧 RAW crop 只迁移一次；fallback alpha 在 BiRefNet 可用后续期并替换文件。
- `tests/domain_behavior.rs`：session 统计与旧格式 commit、describe/embed 批处理的部分失败、face 几何与 skin seed、render 的转向/色彩/曲线/导出色彩空间/镜头几何/蒙版 warp、retouch 的 alpha 规划与 heal、stack 的曝光比与 affine，以及 XMP merge、recipe library portable 和 malformed listing；本轮追加外部 brush/AI XMP、压缩 `.acr` brush table、TIFF ICC、manual perspective 双轴、HDR shoulder 和全局 render gap 披露。
- `tests/properties.rs`：worker 计划边界、手动畸变与 mask warp 的往返、非恒等 warp 的移动性质、clamp 幂等、XMP 标量控件往返，以及镜头 profile 字段保留。
- `tests/decode_behavior.rs`：用 rawler DNG writer 合成无 embedded preview 的 16×8 RAW，验证 `decode_raw_turned` 的 neutral develop 回退保留 90° 转向，preview 和 Meta 同步为 8×16。
- `tests/denoise_behavior.rs`：零强度 identity、float mosaic 不可适用、缺失 RAW denoise sidecar；本轮追加有效 16-bit sidecar 产物采用，以及 timeout 后 buffer 不变和进程组清理。
- `tests/fit_behavior.rs`：fit 预算随强度单调、joint reading 按值匹配且无证据时 abstain、明显色彩差异非零、semantic region 缺失/重叠/边界拒绝、最多返回请求数量的 region、bitmap 预算边界；退化 pair 的完整 fit 和 `fit_recipe_from` 保留 calibration base 作为显式慢 gate。
- `tests/fit_workflow.rs`：合成纹理对真实执行 `fit_recipe`，比较实际渲染预览与目标的误差；`fit_recipe_from` 成功路径保留 calibration base，并验证带 calibration base 的拟合 preview 不恶化；缺失 sidecar 时真实执行 `fit_zoned`，验证 `ZONED_UNAVAILABLE`、可用 fallback 渲染和 claimed raster 释放；fake sidecar 成功时验证 sky zone 以 raster-backed mask 交付。
- `tests/library_behavior.rs`：同一参考库目录连续 build 时复用未变化 embedding，只在新增照片时再次调用 embedding sidecar；RAW sidecar 只有真实渲染接近 finished photo 时才作为 settings 附加。
- `tests/serve_behavior.rs`：真实 `autoshade serve` 子进程、页面 session token 和 listing generation 下，`/api/export` 发布完整登记文件，`/api/download` 流式返回完整图像且不留下持久交付。
- `tests/mcp_behavior.rs`：真实 `autoshade mcp` stdio 子进程完成 initialize、tools/list 和 frame.read；合法根目录文件可读，根目录外路径被拒绝。
- `tests/judge_behavior.rs`：fake `complete.mjs` 边界下，合法 judge JSON 被解析，失败结果保持 typed error 且不泄漏分析密钥。
- `tests/generative_behavior.rs`：fake Comfy CLI 的 env/upload/run/download 协议下，成功生成发布完整 PNG，失败不留下 partial 输出。
- `src/bin/gui/tests.rs`：busy develop 的 `WouldBlock` 与真实 I/O 失败显示不同用户状态，前者保留重试语义。
- `src/bin/gui/tests.rs`：打开/拟合 stamping 只补缺失 calibration，不覆盖用户已有 as-shot WB、base curve 或 lens profile。

这些 case 覆盖的是公开行为；`migrate_legacy`、`with_develop_lock`、HTTP/MCP、真实 RAW、fit 语料等慢路径继续保留在下方 checklist。

## 选材审计

这不是覆盖率台账，而是记录每一批为什么入选、由什么行为证据验收：

| 批次 | 选材理由 | 已补行为 | 状态 |
| --- | --- | --- | --- |
| 1 | 数据丢失/覆盖风险最高，且 Store 是多个入口共享的契约 | backup、commit、orphan recovery、variants、snapshot、XMP sidecar | 完成 |
| 2 | 输出路径和 RAW/baked 分类会直接决定错误目标或错误解码入口 | preflight、扩展名分类 | 完成 |
| 3 | 核心 fit 的 evidence/abstention 是后续 zone fit 和报告的基础 | evidence ranges、same-frame aspect、structure abstention | 完成 |
| 4 | 旧口径把“低风险外围”当成可永久跳过，导致公开纯逻辑没有进入测试范围 | session/describe/embed 解析、face、render 原语、retouch、stack、XMP merge、recipe library、store clear/versions | 完成 |
| 5 | 核心工作流仍缺少公开行为保护 | legacy migration、`fit_zoned`、`pipeline` write/commit、GUI 动作/面板 | 进行中 |

下一批不按“哪个文件未覆盖行最多”选，而按公开行为、失败后果和用户工作流排序。风险只决定先后，不把任何低风险行为永久排除；慢路径使用显式 gate，但不从选材中删除。

### [x] `fit_zoned` segmentation 失败回退
- 公开入口：`fit_zoned::fit_recipe_zoned`，segmentation 缺失/失败时应回退到 global fit，并释放 claimed raster。
- Case：`an_unavailable_segmentation_sidecar_delivers_a_global_fit_and_releases_its_claimed_mask`（`tests/fit_workflow.rs`，显式慢 gate）。
- Premise：4×4 合成纹理对、真实 `OwnedRaster::claim`、不存在的 sidecar 脚本。
- Act：调用公开入口 `fit_zoned::fit_recipe_zoned`，再用返回的 recipe 真实渲染。
- Assert：报告包含 `ZONED_UNAVAILABLE`；交付预览比中性渲染更接近目标；失败后 claimed mask 文件已释放；fallback recipe 不再引用该文件。
- Mutation 证据：删除 `mask_path.remove()` 时，mask 断言变红；把 `ZONED_UNAVAILABLE` 换成其他 key 时，披露断言变红；把 fallback recipe 重置为默认 recipe 时，真实渲染断言变红。
- 独立 mutation：最终测试代码上 `cargo mutants` 对 `segment_both` 的 `Ok((Default::default(), Default::default()))` mutant 报 `1 caught`，证据目录为 `/private/tmp/autoshade-fit-fallback-mutation3`。
- Gate mutation：用只含 `tests/fit_workflow.rs` 的 diff 实测 `mutation_gate.py`；没有 `AUTOSHADE_MUTATION_FILES` 时失败，指定 `src/fit_zoned/attach.rs`、`--re` 和 `--test-arg=-- --test-arg=--ignored <case>` 后报 `1 caught`（`/private/tmp/autoshade-mutation-gate-e2e`）。
- Gate：`AUTOSHADE_FIT_TESTS=1` 的慢门禁；默认快套件不运行。

### [x] `fit_zoned` 成功 segmentation 的 zone 交付
- 公开入口：`fit_zoned::fit_recipe_zoned`，fake segmentation sidecar 返回真实天空 mask。
- Case：`a_successful_zoned_fit_attaches_a_sky_mask_to_the_delivered_recipe`（`tests/fit_workflow.rs`，显式慢 gate）。
- Premise：64×64 合成蓝/绿 source、对应高亮 target、真实 `OwnedRaster::claim`、fake executable 复制天空 mask。
- Act：调用公开入口 `fit_recipe_zoned`。
- Assert：交付 recipe 含 raster-backed zone，source mask 文件保留。
- Mutation：让 `segment_both` 强制失败时，raster-backed zone 断言变红；已用 release 实测。
- Gate：`scripts/fit_battery.sh`。

## 覆盖率诊断快照

覆盖率报告只用于发现未触达的模块，不设目标、不作为完成条件，也不决定测试优先级。当前最明显的未触达区域是 `fit_zoned`、`fit`、`serve`、`mcp`、`generative`、`retouch` 和 GUI 动作/面板模块；下一批测试按核心行为与风险选择。

## 测试政策 gate

这里的“lint”不是产品架构 clippy，而是测试代码自己的政策 gate。它的价值只有一个：把“测试不能读源码、不能猜时序、不能依赖全局状态”变成机器可执行，避免未来测试重新变成实现锁。不是所有规则都值得新增 gate；下面只保留有明确失败模式的部分。

### [x] 测试源码扫描禁令
- 位置：`tests/product_behavior.rs`、`src/bin/gui/tests.rs`。
- 证据：当前两个文件没有 `include_str!`、`include_bytes!`、`gui_source`、`read_dir` 或源码 `read_to_string`；`clippy.toml` 没有对应规则，因此由 `check_test_policy.py` 机械执行。
- 价值：防止测试重新通过读 `.rs`、数函数、查源码标记来证明“功能存在”。
- Gate：`scripts/check_test_policy.py` 扫描测试文件中的源码嵌入、`gui_source`、`source_text`、`source_before_tests`、源码 `contains("fn ...")` 和 `assert!(true)`。
- Mutation：在测试中加入 `include_str!` 指向任意 `.rs`，gate 必须失败；脚本的 `--self-test` 已覆盖该反例。

### [x] 测试时序与全局状态禁令
- 位置：`tests/product_behavior.rs`、`src/bin/gui/tests.rs`。
- 证据：当前测试没有 `thread::sleep`；环境变量写入收敛到测试支持模块的 `ScopedEnv`，由进程内 mutex 串行化并在 `Drop` 中恢复。
- 价值：并发测试必须用 channel/condvar/timeout，临时目录必须按进程/用例隔离；否则测试会偶发失败并污染其他 case。
- Gate：`check_test_policy.py` 拒绝裸 `thread::sleep`、`tests/support` 之外的 `set_var/remove_var`，以及 `temp_dir().join("固定名")`；`--self-test` 同时验证拒绝和允许样例。
- Mutation：把 channel 等待改成 `thread::sleep`，gate 必须失败。

### [x] `cfg(test)` 模块边界
- 位置：`src/bin/gui/main.rs:96`。
- 证据：当前是 `#[cfg(test)] mod tests;`；测试模块由编译器条件排除，不进入发布二进制。
- 价值：已有编译条件，不新增重复 gate；重复扫描只会制造维护负担。
- 结论：不需要新增 lint。

### [x] `logic/` 的无头规则
- 位置：`clippy.toml`、`src/bin/gui/main.rs:75`。
- 证据：`logic` 模块有 `#[deny(clippy::disallowed_types, clippy::disallowed_methods)]`，规则已经由 clippy 执行。
- 价值：规则目标是 `logic/`，不是测试目标。测试可以使用 egui 来验证渲染结果，禁止测试使用 egui 反而是错的。
- 结论：不新增测试侧 lint。

### [x] 字体与 i18n 源码扫描
- 位置：`scripts/subset_gui_fonts.py`、`scripts/check_text.py`。
- 证据：字体子集和文本资源已有脚本 gate，发布电池和 CI 会运行。
- 价值：这是资源/生成物一致性 gate，不应回到行为测试。
- 结论：已覆盖，不新增 lint。

### [x] UI 源码扫描改成可见结果测试
- 位置：旧测试 `the_ai_title_outranks_its_folds...` 中的 `.separator()` 扫描，以及 `the_color_mixer...` 中的 `set_range(` 扫描。
- Range 部分：`changing_a_point_colour_range_rebuilds_all_three_windows` 断言 `set_range` 同时重建 hue/sat/lum 三个窗口；把 `set_range` 改成只存数值时 case 变红。
- 标题/分隔线部分：`the_panel_title_renders_larger_than_group_captions_with_a_short_rule` 用 headless egui rendered shapes 断言 panel title 字号大于 group caption，并存在 28px 短规则线。
- Mutation：把标题从 heading 改成 small 时字号断言变红；把规则线长度改成 0 时规则线断言变红；均已用 release 实测。

## 旧架构测试替代审计

这一节只记录被删除的架构/结构约束。它们不能重新写成源码扫描测试；只有类型、可见性、
rustc 或 Clippy 有真实执行者时才能勾选。自建源码正则 gate 已删除，因此下列项目重新
打开：

- [ ] `every_paint_buffer_mutation_uses_the_presence_notification_door`：补可见 overlay
  refresh 行为，或把通知做成不可绕过的 mutation API。
- [ ] `the_gui_only_offers_controls_the_engine_renders`：需要 typed control registry，
  不能扫描字段赋值文本。
- [ ] `every_raw_image_door_measures_the_plane_first`：逐公开 decode door 验收 plane
  ceiling，或用类型封装未计费 decoder。
- [ ] `the_one_raw_develop_funnel_charges_the_ceiling_before_it_decompresses`：验收真实
  decompression 前的资源拒绝，不能扫描调用顺序。
- [ ] `the_servers_heavy_handlers_take_the_one_library_slot`：用并发 HTTP 行为验收共享
  full-resolution 上限。
- [ ] `the_generative_fill_holds_the_section_only_for_its_local_phases`：用并发/资源行为
  验收 slot 释放与重取。
- [ ] `healing_takes_the_section_where_it_allocates`：用 heal 资源上限行为验收。
- [ ] `the_three_open_paths_share_one_base_look_entry`：比较 pipeline、serve、GUI 的公开
  delivered base look。

### [ ] `every_baked_decode_line_says_why_it_is_allowed`
- 旧位置：`src/render/tests/sources_and_base_look.rs`；原测试要求每个 `load_image` consumer 有允许标记。
- 复核结论：原 `baked-load-image-reason` 只要求前 12 行出现特定注释。注释能够撒谎，
  重排也会误报；它不能证明输入不是 RAW，不满足稳定机械验收，已从自动 gate 删除。
- 当前状态：评审项。若该约束仍然必要，应通过 RAW / baked 类型或不可绕过的公开入口
  表达，再由编译器验收；不得恢复源码注释扫描测试。

### [ ] `the_engine_renders_the_field_from_the_recipe_and_never_calls_the_analyzer`
- 当前状态：需要 Rust module/type visibility 边界；token 扫描不是执行者。

### [ ] `linear_ramp_has_a_single_definition`
- 当前状态：单定义/文件位置是源码形状；保留公开 coverage 等价性行为，或用类型共享实现。

### [ ] `every_store_read_goes_through_the_capped_reader`
- 当前状态：逐 Store 公开读取验收 size refusal，或用仅暴露 capped reader 的类型边界。

### [x] `no_test_prints_its_own_skip_line`
- 旧位置：`src/lib.rs` 测试；原测试扫描 `src/`，禁止手写 `"SKIP..."`。
- 当前状态：由 `scripts/check_test_policy.py` 的 `self-written skip line` 规则机械拒绝。
- 证据：在 `tests/process_behavior.rs` 临时加入 `println!("SKIP: synthetic policy probe")` 时，gate 返回 `self-written skip line`；恢复后 gate 通过。
- 应归：测试政策 gate，不是产品行为测试。

### [x] `test_fixture_dirs_are_process_unique`
- 旧位置：`src/lib.rs` 测试；原测试扫描测试代码中的固定临时目录。
- 当前状态：`check_test_policy.py` 拒绝固定 `temp_dir().join("...")`，共享 `TempTree` 使用 pid + atomic 序号隔离每个 case。
- 证据：self-test 的 `fixed temp directory` 反例被拒绝；正式扫描 15 个测试文件通过。
- 应归：测试政策 gate，不是产品行为测试。

### [x] `the_web_preview_finishes_through_the_engines_own_tail`
- 旧位置：`src/render/finish/tests.rs`；原测试检查 web preview 使用 `frame_and_finish`、`CropPolicy::Keep`，且没有重复 geometry chain。
- 当前状态：`a_web_preview_keeps_the_full_frame_for_slider_feedback` 通过真实 HTTP `/api/develop`，断言带 crop recipe 的返回 JPEG 仍是整帧 16×12。
- Mutation：把 `CropPolicy::Keep` 改成 `Cut` 时，返回尺寸变为 8×6，case 变红；已用 release + ignored HTTP case 实测。
- Gate：slow HTTP server；默认快套件不运行，`cargo test --test serve_behavior -- --ignored` 显式执行。

## Store 与数据安全

### [x] `backup_saved_develop`
- 位置：`src/store/backup.rs:34`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`saving_a_develop_backs_up_the_previous_version_before_overwrite`
- Premise：临时 store root、一张照片、已有 develop 和旧版本快照。
- Act：保存一个新 recipe，触发备份，再读取旧快照。
- Assert：旧 recipe 字节可恢复；新 recipe 指向新内容；备份失败时 live recipe 不变。
- Mutation：跳过备份直接写 live recipe，测试必须红。
- Gate：fast。
- 当前测试：`a_programmatic_backup_preserves_the_previous_recipe_as_a_version`、`a_backup_that_cannot_freeze_a_raster_leaves_the_live_recipe_untouched`。

### [x] `commit_develop`
- 位置：`src/store/commit.rs:77`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_develop_commit_publishes_recipe_pixels_and_variants_as_one_generation`
- Premise：临时 store 中的 recipe、pixels、variants 三个成员。
- Act：调用一次 `commit_develop`，然后读取目录。
- Assert：三个成员同时可见；任一成员写入失败时不留下半套 generation。
- Mutation：提交两个成员后跳过第三个，测试必须红。
- Gate：fast；失败注入只使用临时目录。
- 当前测试：`a_develop_commit_publishes_recipe_pixels_and_variants_together`、`a_commit_with_an_oversized_member_leaves_no_partial_generation`。

### [x] `recover_orphan_baks`
- 位置：`src/store/snapshot.rs:98`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`an_orphan_backup_is_recovered_when_the_live_file_is_missing`
- Premise：只有 `.bak`、没有 live recipe 的 develop 目录。
- Act：启动恢复读取。
- Assert：live 文件恢复为 `.bak` 内容；已有 live 文件不被覆盖。
- Mutation：删除恢复步骤，测试必须红。
- Gate：fast。
- 当前测试：`an_orphan_recipe_backup_is_restored_when_the_live_file_is_missing`、`a_live_recipe_is_not_replaced_by_an_orphan_backup`、`orphan_pixel_and_variant_backups_are_restored_together`。

### [x] `read_develop_snapshot`
- 位置：`src/store/snapshot.rs:196`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_develop_snapshot_reads_recipe_xmp_and_pixel_source_together`
- Premise：同一 develop 目录中的 recipe、XMP、pixels source 和 sidecar。
- Act：读取 snapshot。
- Assert：recipe 内容、XMP 文本、pixel source 和 revision 一致；缺文件有明确状态。
- Mutation：忽略 XMP 或 pixel source，测试必须红。
- Gate：fast。
- 当前测试：`a_develop_snapshot_reads_recipe_projection_and_pixel_source_together`、`an_empty_develop_snapshot_reports_absence_without_an_error`。

### [x] `read_variants_checked`
- 位置：`src/store/variants.rs:191`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`an_unresolved_variants_record_is_not_treated_as_an_empty_strip`
- Premise：临时目录中的 malformed、future-version、未知 kind 和逃逸路径 `variants.json`。
- Act：读取 checked 状态并尝试保存。
- Assert：状态是 `Unresolved`；原文件字节不变；保存被拒绝。
- Mutation：把 `Unresolved` 当 `Absent`，测试必须红。
- Gate：fast。
- 当前测试：`an_unresolved_variant_record_is_not_overwritten_by_a_save`、`a_variant_origin_outside_the_develop_directory_is_unresolved`、`a_malformed_variant_record_is_not_treated_as_absent`、`an_unknown_variant_kind_is_not_treated_as_absent`。

### [x] `variants_member`
- 位置：`src/store/variants.rs:382`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`updating_one_variant_preserves_unknown_fields_and_the_active_card_name`
- Premise：已有 `variants.json`，带未知字段、active name 和 background variant。
- Act：对一个成员做 read-modify-write。
- Assert：未知字段仍在；其他卡片不变；active 位置和名称保持。
- Mutation：序列化时丢掉未知字段，测试必须红。
- Gate：fast。
- 当前测试：`changing_the_active_variant_kind_preserves_unknown_fields_and_its_name`。

### [x] `migrate_legacy`
- 位置：`src/store/migrate.rs:20`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_legacy_recipe_directory_is_migrated_without_losing_the_sidecar`
- Premise：旧 `out/<stem>.recipe.json` 和旧 XMP 命名。
- Act：执行迁移并重新打开。
- Assert：新 develop 可读；旧 sidecar 被保留或明确迁移；无重复版本号。
- Mutation：只迁移 recipe、不迁移 XMP，测试必须红。
- Gate：fast。
- 当前测试：`a_legacy_recipe_and_xmp_are_migrated_into_the_develop_store`。

### [x] `with_develop_lock` 的同进程 `NoWait` 竞争
- 位置：`src/store/lock.rs:42`。
- Case：`a_nowait_develop_lock_refuses_while_another_thread_holds_it`
- Premise：一个线程持有 `Wait` lock，另一个线程请求 `NoWait`；channel 协调，不使用 sleep。
- Act：调用公开入口 `with_develop_lock`。
- Assert：`NoWait` 立即返回 `WouldBlock`；持锁线程释放后正常结束。
- Mutation：让 `NoWait` 绕过锁直接执行回调时，测试在 `WouldBlock` 断言处变红；已用 release 实测。
- Gate：fast。

### [x] `with_develop_lock` 的跨进程写入串行化
- Case：`a_develop_lock_serializes_across_processes`，worker 为 `lock_worker_holds_the_develop_lock_until_released`。
- Premise：父测试启动同一测试二进制的 worker 子进程，worker 持锁后通过 stdout 发 `LOCKED`，通过 stdin 等待释放。
- Act：父进程在 worker 持锁时请求 `NoWait`；释放后再次请求 `Wait`。
- Assert：worker 持锁期间 `NoWait` 返回 `WouldBlock`；释放后 `Wait` 成功。
- Mutation：让 `NoWait` 绕过锁时，测试在 `WouldBlock` 断言处变红；已用 release 实测。
- Gate：fast；worker 为 ignored helper，只由父测试启动。

## Pipeline、CLI、HTTP、MCP

### [x] `produce_recipe` 的缺失 key 前置拒绝
- 位置：`src/pipeline.rs:261`。
- Case：`produce_recipe_without_a_harness_key_refuses_before_decoding`
- Premise：无效 RAW 字节、临时 store、空 `DEEPSEEK_API_KEY`。
- Act：调用公开入口 `produce_recipe`。
- Assert：返回 `no key for the pi harness`；无效 RAW 未被解码；输入字节不变。
- Mutation：绕过 key 检查时，错误文案断言变红；已用 release 实测。
- Gate：fast。

### [x] `produce_recipe` 的正常结果与 harness 结果一致
- Case：`a_fresh_analysis_returns_the_deferred_harness_recipe_and_verdict`。
- Premise：合成 preview、fake Node 可执行文件写回 deferred result 和 summary，临时 config 与 store。
- Act：调用公开入口 `produce_recipe`，走真实 `run_deferred` 边界。
- Assert：返回 recipe 的公开字段、rationale、verdict 与 result 一致；notes 为空。
- Mutation：把返回 recipe 改成 `EditRecipe::default()` 时，recipe 字段断言变红；已用 release 实测。
- Gate：fast；只 stub 外部 Node 进程，不 mock 被测函数。

### [x] `migrate_recipe_coord_frame` 的 baked 分支
- 位置：`src/pipeline.rs:2104`。
- Case：`a_baked_image_with_legacy_coordinates_is_stamped_without_reframing`
- Premise：旧 `coord_era` recipe、baked 图片路径。
- Act：调用公开入口 `pipeline::migrate_recipe_coord_frame`。
- Assert：只把 era 升到当前值，不尝试 reframe，返回 `None`。
- Mutation：删除 baked 分支的 era stamp 时，测试必须红；已用 release 实测。
- Gate：fast。

### [x] `migrate_loaded_recipe` 的 RAW 几何迁移
- Case：`an_old_raw_recipe_migrates_its_crop_once_for_the_exif_turn`。
- Premise：带 `Orientation=Rotate90` 的合成 DNG、旧 `coord_era` recipe、旧 crop。
- Act：调用公开入口 `migrate_loaded_recipe` 两次。
- Assert：crop 按 EXIF 转向映射到显示帧；era 更新；第二次调用不再移动。
- Mutation：把 `turn` 强制为 `None` 时，reframe 断言变红；已用 release 实测。
- Gate：fast；合成 DNG 由 rawler writer 构造。

### [x] `write_xmp`
- 位置：`src/pipeline.rs:3728`。
- Case：`writing_an_xmp_projection_publishes_a_readable_sidecar`、`an_unmergeable_sidecar_is_regenerated_with_a_loss_note`、`a_failed_xmp_publish_leaves_the_previous_projection_intact`。
- Premise：临时照片、recipe、可写 develop、不可合并的旧 XMP、只读 develop 目录。
- Act：调用公开入口 `pipeline::write_xmp` / `write_xmp_at`。
- Assert：成功时 sidecar 可解析；不可合并时重新生成并披露；发布失败时旧 projection 字节不变。
- Mutation：把 `durable_write` 换成原地 `std::fs::write` 时，失败发布用例变红；已用 release 实测。
- Gate：fast。

### [x] `commit_analyzed_develop`
- 位置：`src/pipeline.rs:3850`。
- Case：`an_analyzed_raw_commit_publishes_recipe_and_xmp_together`、`an_analyzed_baked_commit_clears_the_lightroom_projection`、`an_analyzed_commit_after_its_commit_point_recovers_when_xmp_conflict_is_removed`。
- Premise：RAW/baked develop、XMP 目录冲突、临时 store。
- Act：提交 analyzed develop，修复冲突后再次提交。
- Assert：正常提交同时发布 recipe/XMP；baked 清理 projection；提交点后失败明确报告 `save IS committed`，修复后恢复完成。
- Mutation：跳过 XMP 写入时，冲突恢复用例在“首次必须失败”断言处变红；已用 release 实测。
- Gate：fast。

### [x] `preflight_out`
- 位置：`src/pipeline.rs:4558`。
- Case：`a_directory_output_target_is_refused_before_rendering`、`an_existing_photo_at_the_output_path_is_not_replaced`、`a_new_output_target_creates_its_parent_directory`、`a_read_only_output_directory_is_refused_before_rendering`、`an_output_symlink_to_a_photo_is_refused`。
- Premise：目录目标、已有照片、只读目录、指向源照片的 symlink 和合法新目标。
- Act：调用公开入口 `pipeline::preflight_out`。
- Assert：危险目标返回错误且已有字节不变；合法目标创建父目录。
- Mutation：把 `guard_readonly(out, src)?` 改成忽略结果时，已有照片和 symlink 用例变红；已用 release 实测。
- Gate：fast。

### [x] `serve::api_export`
- 位置：`src/serve.rs:2454`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_http_export_publishes_one_complete_registered_file`。
- Premise：真实 `autoshade serve` 子进程、临时端口、临时 source 和 delivery root、页面 token 与 listing generation。
- Act：请求 `/api/export`。
- Assert：返回已登记路径，文件存在且是完整 JPEG。
- Mutation：发布前截断临时文件时，JPEG 完整性断言变红；已用 release 实测。
- Gate：slow；显式 `cargo test --test serve_behavior -- --ignored`。

### [x] `serve::api_download`
- 位置：`src/serve.rs:2524`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_http_download_streams_one_complete_image_without_a_persistent_delivery`。
- Premise：真实 `autoshade serve` 子进程、临时端口、临时 source 和 delivery root、页面 token 与 listing generation。
- Act：请求 `/api/download` 并读取响应流。
- Assert：响应是完整 JPEG；delivery 目录没有持久交付文件。
- Mutation：发布前截断临时文件时，JPEG 流完整性断言变红；已用 release 实测。
- Gate：slow；显式 `cargo test --test serve_behavior -- --ignored`。

### [x] `mcp::Server::run`
- 位置：`src/mcp.rs:67`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`an_mcp_client_can_list_tools_and_read_a_photo_under_its_root`、`an_mcp_frame_read_refuses_a_path_outside_its_root`。
- Premise：临时根目录、真实 `autoshade mcp` 子进程、合法 PNG 和根目录外 PNG。
- Act：通过 stdio 发送 initialize、tools/list、frame.read 请求。
- Assert：工具列表包含 `frame.read`；合法文件返回图像内容；逃逸路径返回 `bad_path` 失败。
- Mutation：清空 tools/list 时，工具发现断言变红；去掉 resolve 的 root 前缀检查时，逃逸断言变红；均已用 release 实测。
- Gate：fast；真实 CLI stdio 子进程，不使用外部模型。

## Fit 与 Zoned Fit

本节中 `derive_*`、`tile_evidence`、`eligible`、`boundary_line_rims` 是私有 helper，不能直接写测试。它们只作为定位信息；验收入口必须是 `fit_recipe_zoned` 的公开结果、可见 disclosure 或交付 recipe，不能以 helper 被调用为验收。

### [x] `fit_recipe_from` 的退化输入保留 calibration base
- 位置：`src/fit/solve.rs:45`。
- Case：`a_degenerate_pair_keeps_the_calibration_base_in_the_refused_recipe`
- Premise：退化黑帧、带 `as_shot_k` 和 `base_curve` 的 calibration base。
- Act：调用公开入口 `fit_recipe_from`。
- Assert：拒绝拟合仍保留 base 的 calibration 字段，并披露 `FIT_DEGENERATE`。
- Mutation：把拒绝路径的 `..base.clone()` 改成默认 recipe 时，测试必须红；已用 release 实测变红。
- Gate：`scripts/fit_battery.sh`；默认快套件不运行。

### [x] `fit_recipe_from` 的正常结果不恶化
- Case：`a_fit_from_a_calibration_base_does_not_worsen_the_delivered_preview`。
- Premise：合成 source/target 图像对、带 calibration base 的 recipe。
- Act：调用公开入口 `fit_recipe_from`，再用返回 recipe 真实渲染 source。
- Assert：拟合后的 preview 误差不高于 base preview；无解时不写坏 recipe。
- Mutation：把返回 recipe 强制加 5 EV 时，preview 误差断言变红；已用 release 实测。
- Gate：slow；`scripts/fit_battery.sh`。

### [x] `evidence_model_paired`
- 位置：`src/fit/evidence.rs:571`。
- Case：`evidence_model_with_no_present_target_pixels_has_no_population`，另有 `a_structured_pair_spans_multiple_luma_ranges_while_a_flat_pair_stays_in_one`。
- Premise：64 个合成像素，target presence 全为 0；以及结构化/平坦两类正常输入。
- Act：调用公开入口 `fit::evidence_model_paired`。
- Assert：无 target 像素时 `population == 0` 且 source/target weights 全为 0；正常输入仍产生可用 luma 范围。
- Mutation：把 presence 过滤改成全 1 时，population 断言变红；已用 release 实测。
- Gate：fast。

### [x] `derive_luminance_bands`
- 位置：`src/fit_zoned/range.rs:611`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`luminance_bands_cover_only_the_residual_runs_that_have_evidence`
- Premise：带残差 run、背景和边界样本的合成帧。
- Act：派生亮度带。
- Assert：带边界不越出证据；空 run 不生成 band；顺序稳定。
- Mutation：忽略证据阈值，测试必须红。
- Gate：fast。
- 拒绝路径：`a_zoned_fit_reports_unsupported_luminance_and_colour_bands_without_attaching_them` 断言低 population/零结构证据的 `RANGE_ABSTAINED`，并确认没有附带 luminance range mask。
- 正面路径：`a_zoned_fit_attaches_a_measured_luminance_range_when_a_paired_look_improves` 使用 checked-in canyon showcase 的左右半幅作为真实 paired look，真实 segmentation 失败回退交付 `RangeMask::Luminance`，并披露 `RANGE_ATTACHED` 的 before > after（0.165→0.014）。
- Mutation：关闭 luma evidence gate 时拒绝路径变红；在 `derive_luminance_bands` 返回前清空 bands 时正面 case 在缺少 luminance range mask 处变红；均已用 release 实测。

### [x] `derive_colour_bands`
- 位置：`src/fit_zoned/range.rs:1392`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`colour_bands_reject_foreign_hues_and_keep_the_measured_band`
- Premise：目标色相、外来色相和低饱和像素。
- Act：派生 colour bands。
- Assert：外来色相被拒绝；目标色相产生 band；空结果有 abstention。
- Mutation：移除 foreign-hue gate，测试必须红。
- Gate：fast。
- 拒绝路径：`a_zoned_fit_reports_unsupported_luminance_and_colour_bands_without_attaching_them` 断言 Orange/Aqua/Blue 的 `COLOUR_RANGE_ABSTAINED` 零结构证据原因，并确认没有附带 colour range mask。
- 正面路径：`a_zoned_fit_attaches_a_measured_colour_range_when_one_hue_band_improves` 使用 checked-in showcase 图生成带特定亮度变化的 target，真实 segmentation 失败回退交付 `RangeMask::Color`，并披露 `COLOUR_RANGE_ATTACHED` 的 before > after。
- Mutation：关闭 hue evidence gate 时拒绝路径变红；在 `derive_colour_bands` 返回前清空 proposals 时正面 case 在缺少 colour range mask 处变红；均已用 release 实测。

### [x] `tile_evidence`
- 位置：`src/fit_zoned/spatial.rs:156`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_tile_without_enough_texture_is_not_eligible_for_a_fit`
- Premise：平坦 tile、有纹理 tile、parent residual。
- Act：读取 tile evidence。
- Assert：平坦 tile 被拒绝；有纹理 tile 可进入候选。
- Mutation：删除纹理阈值，测试必须红。
- Gate：fast。
- 当前公开工作流证据：`a_zoned_fit_discloses_supported_tiles_and_withholds_unsupported_range_bands` 观察到 `TILE_ELIGIBLE` 与 `TILE_ABSTAINED`；`a_flat_zoned_pair_offers_no_spatial_tile_or_boundary_charge` 证明平坦 pair 不会产生 `TILE_ELIGIBLE`。
- Mutation：把 paired pixel 数强制为 0 时，有纹理 case 在 `TILE_ELIGIBLE` 断言处变红；已用 release 实测。

### [x] `eligible`
- 位置：`src/fit_zoned/spatial.rs:227`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_tile_that_regresses_its_parent_is_rejected`
- Premise：改善 tile 和回归 tile 的 reading。
- Act：调用 `eligible`。
- Assert：改善 tile 通过；回归 tile 返回明确拒绝原因。
- Mutation：只检查 tile 内部误差，测试必须红。
- Gate：fast。
- 审计结论：`eligible` 是私有 helper，正常公开工作流不区分 parent-residual 拒绝与其他 tile abstention；直接测试它只能新增生产 seam，违反私有 helper 不单独立项的原则。
- 公开覆盖：`a_zoned_fit_discloses_supported_tiles_and_withholds_unsupported_range_bands`、`a_flat_zoned_pair_offers_no_spatial_tile_or_boundary_charge` 和 `a_zoned_fit_attaches_a_measured_luminance_range_when_a_paired_look_improves` 覆盖 tile 进入、拒绝、平坦不误报及真实 paired-look 结果。
- Mutation：`tile_evidence` paired pixel 数强制为 0 时，`TILE_ELIGIBLE` 断言变红；已用 release 实测。

### [x] `boundary_line_rims`
- 位置：`src/fit_zoned/boundary.rs:711`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_boundary_reading_charges_a_crossing_only_when_the_rim_changes`
- Premise：合成阶跃边缘、无边缘和单边位移帧。
- Act：读取 boundary rims。
- Assert：阶跃处产生 charge；平坦处不产生；样本顺序稳定。
- Mutation：忽略 rim 变化，测试必须红。
- Gate：fast。
- 当前公开工作流证据：`a_feathered_semantic_region_boundary_is_measured_before_attachment` 通过真实 multi-class feathered manifest 触发 `TransitionBand`，断言 `ZONE_BOUNDARY_PASSED` 的 `transitions > 0`；`a_flat_zoned_pair_offers_no_spatial_tile_or_boundary_charge` 提供平坦 pair 的零 charge 对照。
- Mutation：把 `boundary_line_rims` 直接 `return` 时，feathered case 在 `ZONE_BOUNDARY_PASSED` 断言处变红；已用 release 实测。

## AI 与外部 Sidecar

### [x] `run_sidecar_carrying` 的 timeout 与进程组清理
- 位置：`src/denoise.rs:775`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_timed_out_denoise_sidecar_leaves_the_buffer_untouched_and_reaps_its_group`。
- Premise：临时 fake executable、短 timeout、记录 grandchild pid 的 shell。
- Act：通过公开入口 `denoise_buffer` 进入真实 sidecar bridge。
- Assert：返回 timeout 错误；输入 buffer 不变；grandchild 在有界时间内退出。
- Mutation：把 `run_sidecar_child` 的 kill group 强制为 `None` 时，grandchild 存活断言变红；已用 release 实测。
- Gate：fast（约 1 秒 timeout）；fake executable 只替代外部进程。

### [x] `denoise_mosaic` 的零强度和不可适用路径
- 位置：`src/denoise.rs:661`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_zero_strength_mosaic_denoise_is_the_identity_without_a_sidecar`、`a_float_mosaic_is_not_applicable_to_raw_denoise`、`a_missing_raw_denoise_sidecar_is_reported`
- Premise：合成 RGGB `RawImage`、float mosaic、缺失 raw sidecar。
- Act：调用 `denoise_mosaic`。
- Assert：强度 0 不启动 sidecar 且像素不变；float mosaic 返回 NotApplicable；缺 sidecar 返回明确错误。
- Mutation：把 `<= 0` 改成 `< 0` 时，零强度用例变红；已用 release 实测。
- Gate：fast。

### [x] `denoise_mosaic` 的有效 sidecar 输出
- Case：`a_successful_raw_denoise_adopts_the_same_sized_16_bit_mosaic`。
- Premise：合成 `RawImage`、fake sidecar、有效同尺寸 16-bit grayscale PNG。
- Act：调用公开入口 `denoise_mosaic`。
- Assert：mosaic 尺寸不变；sidecar 的 16-bit 像素被采用；整数采样类型保持。
- Mutation：把返回的 `pixels` 丢掉并写入全零时，首像素断言变红；已用 release 实测。
- Gate：fast；fake executable 只替代外部 sidecar。

### [x] `segment_file` 缺失脚本在创建 artifact 前失败
- 位置：`src/segment.rs:543`。
- Case：`a_missing_segmentation_sidecar_is_reported_before_it_creates_a_mask`
- Premise：临时输入文件、缺失的 sidecar 脚本、未创建的输出路径。
- Act：调用公开入口 `segment_file`。
- Assert：错误明确包含 `segmentation sidecar not found`，输出文件不存在。
- Mutation：绕过脚本存在检查时，错误文案断言变红；已用 release 实测。
- Gate：fast。

### [x] `segment_file` 的 exit-zero 无 artifact 分支
- 位置：`src/segment.rs:543`。模块行覆盖 8.57%，测试调用次数 0。
- Case：`a_segmentation_sidecar_that_exits_zero_without_output_is_a_failure`
- Premise：fake executable sidecar 正常退出 0，但不创建输出文件。
- Act：调用公开入口 `segment_file`。
- Assert：返回 `exited 0 but wrote no output`，输出文件不存在。
- Mutation：忽略 `sidecar_wrote` 结果时，错误文案断言变红；已用 release 实测。
- Gate：fast；fake sidecar 只替代外部进程。

### [x] `should_renew_cached_alpha`
- 位置：`src/segment.rs:833`。模块行覆盖 8.57%，测试调用次数 0。
- Case：`a_fallback_alpha_is_renewed_when_the_primary_segmenter_becomes_available`。
- Premise：真实生成的 alpha、旧 fallback/`birefnet-deps=missing` marker、fake sidecar 的 probe 返回 `ok`，并用第二张不同 alpha 作为续期产物。
- Act：第二次调用公开入口 `resolve_ai_masks`。
- Assert：`renewed == 1`、`cached == 0`，alpha 文件字节确实改变。
- Mutation：把 `should_renew_cached_alpha` 强制返回 false 时，`renewed` 断言变红；已用 release 实测。
- Gate：fast；fake executable 只替代外部 probe/segmenter。

### [x] `resolve_ai_masks` 的缺失 sidecar 分支
- 位置：`src/segment.rs:1160`。
- Case：`resolving_an_ai_mask_with_a_missing_sidecar_reports_unresolved_and_keeps_no_raster`
- Premise：未解析 AI mask、临时 store、缺失 segmentation script、无效源路径。
- Act：调用公开入口 `resolve_ai_masks`。
- Assert：mask 仍无 raster；unresolved 记录 `not at`；不会把无效源伪装成已解析。
- Mutation：绕过缺失脚本检查时，unresolved 文案断言变红；已用 release 实测。
- Gate：fast。

### [x] `resolve_ai_masks` 的 cache hit
- 位置：`src/segment.rs:1160`。模块行覆盖 8.57%，测试调用次数 0。
- Case：`resolving_an_ai_mask_reuses_a_decodable_cached_alpha_without_the_sidecar`
- Premise：fake executable sidecar 生成一次 alpha；随后删除 fake sidecar 和脚本。
- Act：第二次调用公开入口 `resolve_ai_masks`。
- Assert：仍从可解码 cache 复用 alpha，`cached == 1`、`unresolved` 为空；不会重新启动 sidecar。
- Mutation：忽略 cache hit 时，第二次解析变成 unresolved，`cached` 断言变红；已用 release 实测。
- Gate：fast；fake sidecar 只替代外部进程。

### [x] `generative::reimagine`
- 位置：`src/generative.rs:198`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_successful_reimagine_publishes_one_complete_generated_image`、`a_failed_reimagine_does_not_install_a_partial_generated_image`。
- Premise：合成 PNG、fake Comfy CLI 的 env/upload/run/download 四段协议、失败开关。
- Act：调用公开入口 `generative::reimagine`。
- Assert：成功输出是完整 PNG 且有有限 divergence；失败不创建输出文件。
- Mutation：成功路径返回空字节时 PNG 完整性断言变红；失败路径报错前写 partial 时 `!out.exists()` 断言变红；均已用 release 实测。
- Gate：fast；fake executable 只替代 Comfy 外部进程。

### [x] `judge_pair`
- 位置：`src/advisor/judge.rs:390`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_successful_judge_result_is_parsed_through_the_pi_harness_boundary`、`a_judge_failure_is_typed_and_does_not_leak_the_analysis_key`。
- Premise：fake `complete.mjs` Node 进程、合法 judge JSON、失败 result 和分析密钥。
- Act：调用公开入口 `judge_pair`。
- Assert：成功响应解析出 score/decision/critique；失败返回 typed error，文本不含密钥。
- Mutation：把成功 score 改成 0，成功断言变红；把失败文本附加密钥，泄漏断言变红；均已用 release 实测。
- Gate：fast；fake executable 只替代 pi harness 外部边界。

## XMP 投影与合并

### [x] `merge_recipe_into_xmp`
- 位置：`src/xmp/merge.rs:218`。模块行覆盖 5.70%，测试调用次数 0。
- Case：`merging_a_recipe_preserves_an_unowned_sidecar_property`、`a_merged_recipe_reads_back_with_its_exposure`、`merging_the_same_recipe_twice_is_byte_stable`
- Premise：手写外来 XMP，带未知属性和已有 recipe。
- Act：连续 merge 两次。
- Assert：未知属性保留；两次结果语义相同；我们的 owned keys 被更新。
- Mutation：每次 merge 追加额外标记时，第二次输出不再字节稳定，测试变红；已用 release 实测。
- Gate：fast。

### [x] `parse_brush_group`
- 位置：`src/xmp/read_corrections.rs:5`。模块行覆盖 36.23%，测试调用次数 0。
- Case：`an_external_brush_group_preserves_its_strokes_and_dab_stream`。
- Premise：由公开 writer 生成的 `Mask/Aggregate`、`Mask/Paint` 和 dabs，禁用私有 payload 后作为外部 XMP 读取。
- Act：调用公开入口 `xmp_to_recipe`。
- Assert：stroke 数、dabs、value、radius、flow 和 center weight 一致。
- Mutation：在 `parse_dabs` 返回前清空 token stream 时，dabs 断言变红；已用 release 实测。
- Gate：fast；payload 禁用是外部 sidecar 场景，不读取私有 helper。

### [x] `parse_ai_mask`
- 位置：`src/xmp/read_corrections.rs:107`。模块行覆盖 36.23%，测试调用次数 0。
- Case：`an_external_ai_mask_preserves_its_reference_provenance_and_gesture`。
- Premise：由公开 writer 生成的 `Mask/Image`、reference point、provenance 和 gesture，禁用私有 payload 后作为外部 XMP 读取。
- Act：调用公开入口 `xmp_to_recipe`。
- Assert：reference point、mask version、provenance 和 gesture dabs 一致。
- Mutation：在 `parse_ai_mask` 返回前清空 provenance 时，provenance 断言变红；已用 release 实测。
- Gate：fast。

### [x] `parse_mask_brush_payload`
- 位置：`src/xmp/brush_table.rs:490`。模块行覆盖 1.26%，测试调用次数 0。
- Case：`an_aggregate_brush_table_imports_its_compressed_strokes`。
- Premise：合成 `.acr` header/directory/object、Brotli payload、32-byte sync id 和 `Mask/Aggregate` XMP。
- Act：调用公开入口 `xmp_to_recipe_for_photo`。
- Assert：压缩 table 导入为 brush group，sync id、dabs、value、radius、flow 和 center weight 正确。
- Mutation：去掉 payload token 累加时，stroke 导入断言变红；已用 release 实测。
- Gate：fast；fixture 完全合成，不依赖外部模型或真实照片。

### [x] `global_render_gaps`
- 位置：`src/xmp/losses.rs:788`。模块行覆盖 9.09%，测试调用次数 0。
- Case：`a_fully_rendered_global_recipe_has_no_render_gap_disclosures`。
- Premise：包含 exposure、texture、post-crop vignette、grain 的已渲染全局 recipe。
- Act：调用公开入口 `xmp::global_render_gaps`。
- Assert：已渲染控件不产生虚假 gap 披露。
- Mutation：让 `render_gaps_in` 对所有控件报缺口时，空列表断言变红；已用 release 实测。
- Gate：fast。

## Render 与 Export

### [x] `convert_export_color_space`
- 位置：`src/render/export.rs:164`。模块行覆盖 32.60%，测试调用次数 0。
- Case：`srgb_export_leaves_pixels_unchanged`、`a_white_pixel_stays_white_in_display_p3`、`a_saturated_srgb_colour_changes_when_encoded_as_display_p3`
- Premise：合成 RGB 图、sRGB 和宽色域配置。
- Act：转换并读取像素。
- Assert：sRGB 恒等；P3 白色保持；饱和 sRGB 红在 P3 编码中移动且通道非零。
- Mutation：把 P3 映射改成恒等时，饱和色用例变红；已用 release 实测。
- Gate：fast。

### [x] `tag_icc`
- 位置：`src/render/export.rs:268`。模块行覆盖 32.60%，测试调用次数 0。
- Case：`a_delivered_tiff_carries_the_selected_icc_profile`。
- Premise：合成 PNG、sRGB 和 Display P3 两次真实导出、临时 TIFF。
- Act：调用公开入口 `render_to_file`，用 TIFF ICC tag 读回。
- Assert：两个 TIFF 都有 ICC profile，且所选色彩空间产生不同 profile。
- Mutation：把 `tag_icc` 强制为 sRGB 时，profile 不同断言变红；已用 release 实测。
- Gate：fast。

### [x] `stage_and_publish`
- 位置：`src/render/export.rs:248`。模块行覆盖 32.60%，测试调用次数 0。
- Case：`a_staged_export_publish_replaces_the_delivery_once_written`、`a_failed_staged_export_publish_leaves_the_existing_delivery_untouched`
- Premise：已有目标文件；成功写入 staging；编码回调返回错误。
- Act：调用公开入口 `render::stage_and_publish`。
- Assert：成功时一次性替换；失败时旧文件字节不变，staging 被清理。
- Mutation：失败分支不删除 staging 时，目录数量断言变红；已用 release 实测。
- Gate：fast。

### [x] `camera_base_knots`
- 位置：`src/render/base_look.rs:73`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`camera_base_knots_fit_a_known_tone_curve`、`camera_base_knots_refuse_a_frame_smaller_than_the_grid`
- Premise：合成 neutral/camera 图像对和已知曲线。
- Act：估计 knots。
- Assert：非恒等曲线产生至少两个单调 knots；小于 grid 的图片返回 None。
- Mutation：把非恒等结果强制成空 knots 时，非恒等曲线用例变红；已用 release 实测。
- Gate：fast。

### [x] `as_shot_wb`
- 位置：`src/render/as_shot.rs:12`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`as_shot_wb_reads_finite_temperature_and_tint_from_dng_metadata`、`as_shot_wb_returns_none_when_the_dng_has_no_colour_matrix`
- Premise：rawler DNG writer 合成带 `AsShotNeutral`/colour matrix 的 RAW，以及缺 colour matrix 的 RAW。
- Act：调用 `as_shot_wb`。
- Assert：有 metadata 返回有限 Kelvin/Tint 且不等于默认 5500K；缺 colour matrix 返回 None。
- Mutation：固定返回 5500K/0 时，默认 anchor 断言变红；已用 release 实测。
- Gate：fast；合成 DNG fixture。

### [x] `perspective::manual`
- 位置：`src/render/perspective.rs:268`。模块行覆盖 14.80%，测试调用次数 0。
- Case：`a_horizontal_perspective_keystone_changes_the_rendered_frame`、`a_vertical_perspective_keystone_changes_the_rendered_frame`。
- Premise：合成棋盘帧、单轴 keystone recipe。
- Act：调用公开入口 `frame_and_finish`。
- Assert：每个轴的非中性 keystone 都改变交付像素；中性帧保持不变。
- Mutation：把两个 keystone 系数都设为 0 时，两个 case 分别变红；已用 release 实测。
- Gate：fast。

### [x] `hdr::apply`
- 位置：`src/render/hdr.rs:202`。模块行覆盖 7.41%，测试调用次数 0。
- Case：`an_hdr_rendition_compresses_a_highlight_without_lifting_black`。
- Premise：合成浮点帧，含黑点和高于显示范围的亮部，`hdr_max_ev = 2`。
- Act：调用公开入口 `develop_preview`。
- Assert：亮部被 shoulder 压低；黑点仍为零。
- Mutation：把 headroom 强制为 0 时，亮部压低断言变红；已用 release 实测。
- Gate：fast。

## GUI 持久化与状态

### [x] `read_saved_develop`
- 位置：`src/bin/gui/persist.rs:90`。
- Case：`opening_a_saved_recipe_restores_the_develop_without_a_loss_note`。
- Premise：临时 store、可读的 `recipe.json`、非中性 recipe。
- Act：调用恢复入口。
- Assert：返回原 recipe；没有 XMP、mask、全局设置或 clamp 损失披露。
- Mutation：跳过 recipe 恢复分支时，恢复断言变红；已用 release 实测。
- Gate：fast。
- Case：`opening_a_corrupt_recipe_falls_back_to_xmp_and_reports_the_primary_loss`。
- Premise：损坏的 `recipe.json`、可读的 XMP projection。
- Act：调用恢复入口。
- Assert：返回 `Unreadable`、保留 XMP fallback，并带回主文件的错误。
- Mutation：让损坏 recipe 被静默忽略时，`Unreadable` 断言变红；已用 release 实测。
- Gate：fast。
- Case：`a_neutral_recipe_does_not_resurrect_an_older_xmp_develop`。
- Premise：中性 `recipe.json` 和带旧曝光的 XMP。
- Act：调用恢复入口。
- Assert：结果是 `NoopOnly`，旧 XMP 不被恢复。
- Mutation：把中性 recipe 当成可恢复 recipe 时，`NoopOnly` 断言变红；已用 release 实测。
- Gate：fast。
- Case：`an_unreadable_lightroom_sidecar_is_reported_while_the_store_recipe_still_opens`。
- Premise：可读的中央 recipe，以及不可读的 Lightroom sidecar。
- Act：调用恢复入口。
- Assert：中央 recipe 仍打开；`lr_unreadable` 披露 sidecar 损失。
- Mutation：把不可读 sidecar 折成 absent 时，披露断言变红；已用 release 实测。
- Gate：fast。

### [x] `stamp_calibration`
- 位置：`src/bin/gui/persist.rs:682`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`stamping_calibration_preserves_saved_user_values_and_fills_missing_ones`。
- Premise：已有 as-shot WB 的 saved recipe、空 fresh recipe、带 base curve 和 lens profile 的 calibration start。
- Act：调用 `stamp_calibration`。
- Assert：已有 WB 保留；缺失 WB 被补；base curve 和 lens profile 正确写入。
- Mutation：让已有 `as_shot_k` 也被覆盖时，保存值断言变红；已用 release 实测。
- Gate：fast。

### [x] `strip_from_record`
- 位置：`src/bin/gui/persist.rs:776`。
- Case：`restoring_a_variant_record_preserves_background_cards_and_their_identity`。
- Premise：`variants.json` 的 background kind、recipe、origin、id 和 name。
- Act：重建背景卡条。
- Assert：顺序、kind、id、名称、origin 和 recipe 都保留。
- Mutation：恢复时丢掉名称时，名称断言变红；已用 release 实测。
- Gate：fast。
- Case：`a_saved_variant_record_keeps_the_active_card_position_when_matching_live_cards`。
- Premise：活动卡不在 `others` 中，背景卡用 kind + 位置匹配且没有 id。
- Act：按保存记录匹配 live cards。
- Assert：活动卡占用 `active_pos`，两侧背景卡映射到正确位置。
- Mutation：忽略 `active_pos`、直接用 `others` 下标时，后侧卡匹配失败并变红；已用 release 实测。
- Gate：fast。
- Case：`restoring_an_oversized_variant_recipe_reports_the_cards_dropped_by_clamping`。
- Premise：65 个 mask 的背景 recipe，超过 recipe 上限。
- Act：重建背景卡条。
- Assert：recipe 截到 64 个 mask，`ClampSummary` 报告丢弃 1 个。
- Mutation：只 clamp recipe、不累计摘要时，丢弃计数断言变红；已用 release 实测。
- Gate：fast。

### [x] `persist_postponed`
- 位置：`src/bin/gui/workspace.rs:1064`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_busy_develop_is_reported_differently_from_a_real_io_failure`。
- Premise：默认 workspace、`WouldBlock` 与普通 `disk full` I/O 错误。
- Act：调用 `persist_postponed`，读取用户可见 status。
- Assert：`WouldBlock` 显示另一个 AutoShade 进程的重试文案；真实 I/O 错误显示 `disk full`，不伪装成锁冲突。
- Mutation：让 `WouldBlock` 走普通失败分支时，busy 文案断言变红；已用 release 实测。
- Gate：fast；直接验证状态文案，不依赖 worker 或 sleep。

## Library、Face、Platform

### [x] `library::build`
- 位置：`src/library/build.rs:76`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_library_build_reuses_unchanged_embeddings_and_embeds_only_new_photos`。
- Premise：临时成片目录、fake embedding executable、一次初始 build、一次相同 build、一次新增文件 build。
- Act：连续调用公开入口 `library::build`。
- Assert：第二次 build 不启动 embedding sidecar；新增文件只触发一次新调用；原条目 id 保留。
- Mutation：让 `build` 忽略 previous library 时，第二次 build 的 sidecar 计数断言变红；已用 release 实测。
- Gate：fast；fake executable 只替代外部 embedding 模型。

### [x] `best_settings`
- 位置：`src/library/build.rs:218`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_reference_sidecar_is_attached_only_when_it_renders_to_the_finished_photo`。
- Premise：合成 RAW、同源 finished PNG、故意改成高 ΔE 的 finished PNG、默认 XMP sidecar、fake embedding sidecar。
- Act：执行公开入口 `library::build`，读取最终库条目的 `settings`。
- Assert：同源成片附加 settings 且 `match_de <= MATCH_DE_MAX`；高 ΔE 成片不附加。
- Mutation：忽略 ΔE 阈值时，高 ΔE 条目的 `settings.is_none()` 断言变红；已用 release 实测。
- Gate：fast；fake executable 只替代 embedding 模型。

### [x] `face::skin_seed`
- 位置：`src/face.rs:273`。模块行覆盖 22.77%，测试调用次数 0。
- Case：`skin_seed_samples_only_pixels_the_face_mask_holds`、`skin_seed_returns_none_without_face_coverage`
- Premise：已知 RGB reference、软 face mask 和空 mask。
- Act：调用 `skin_seed`。
- Assert：median 和 centroid 正确；无覆盖像素返回 None。
- Mutation：空 mask 返回伪造 seed 时，`skin_seed_returns_none_without_face_coverage` 变红；已用 release 实测。
- Gate：fast。

### [x] `resolve_face_masks`
- 位置：`src/face.rs:569`。模块行覆盖 22.77%，测试调用次数 0。
- Case：`resolving_face_intents_writes_face_and_eyes_rasters_and_reuses_cache`、`an_unusable_face_report_removes_face_intents_and_reports_the_reason`
- Premise：face intent recipe、fake face report、坏 raster。
- Act：解析 face masks。
- Assert：可用 face/eyes mask 被填充且删除 sidecar 后走 cache；坏 report 移除 intents 并记录 reason。
- Mutation：失败时保留无 raster 的 mask，测试必须红；已用 release 实测。
- Gate：fast；sidecar 用 fake executable。

### [x] `KillGroup::kill_tree`
- 位置：`src/lib.rs:158`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`killing_a_sidecar_group_reaps_its_grandchild`
- Premise：父进程启动一个 child，child 再启动 grandchild；使用 channel 和 timeout。
- Act：调用 `kill_tree`。
- Assert：child 和 grandchild 都退出；无 zombie；不会杀死无关进程。
- Mutation：把 `killpg` 改成只 kill group leader 时，grandchild 存活断言变红；已用 release 实测。
- Gate：fast；bounded `yield_now`，不使用 sleep。

### [x] `macos::install_quit_guard`
- 位置：`src/bin/gui/macos.rs:94`。模块行覆盖 0.00%，测试调用次数 0。
- Case：`a_dirty_quit_request_cancels_and_arms_the_in_app_close_prompt`、`a_clean_quit_request_terminates_without_arming_the_close_prompt`、`installing_the_macos_quit_guard_without_a_live_delegate_reports_a_safe_refusal`。
- Premise：未保存状态、干净状态，以及没有 live NSApplication delegate 的 macOS 测试进程。
- Act：调用 `quit::on_terminate_request` 和 `macos::install_quit_guard`。
- Assert：Dirty 返回 `Cancel` 并 arm close request；Clean 返回 `Now` 且不 arm；无 delegate 时安装入口安全拒绝。
- Mutation：把 Dirty 的回复改成 `Now` 时，dirty case 在 `Cancel` 断言处变红；已用 release 实测。
- Gate：前两条跨平台 fast；安装入口在 macOS 编译并运行。
