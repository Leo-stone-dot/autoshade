# 像素操作与请求上限验收

日期：2026-11-23。延续 `HTTP_BEHAVIOR_AUDIT.md`，不替代未完成的 GUI 验收。

## GUI 执行环境

本轮实际查询结果：

- 原生 Computer Use 返回 `Sky Computer Use native pipe startup failed`。
- 只读辅助功能检查 `AXIsProcessTrusted()` 返回 `false`。
- 未启动真实用户 GUI，未修改系统权限，未写入生产测试开关。

因此以下行为继续待完成，不能用源码扫描、私有 helper 或“进程没退出”替代：

| 状态 | 公开行为 | 所需证据 | 当前缺口 |
| --- | --- | --- | --- |
| [ ] | 启动后出现可操作的 AutoShade 窗口 | 隔离 HOME/数据目录下实际启动，观察窗口和画布；移除事件循环时失败 | 原生桌面操作不可用 |
| [ ] | 正常关闭再打开保留用户窗口偏好 | 通过窗口操作改尺寸，正常关闭、重启后观察恢复 | 不能执行窗口操作和正常关闭 |
| [ ] | 自定义数据目录不读写默认偏好 | 两个独立目录通过真实启动产生不同偏好，验证隔离及恢复 | 同上；不伪造内部 prefs 结构 |
| [ ] | 旧偏好迁移、失败回退和平台 opt-out | 对应平台通过启动入口验收文件及可见偏好 | Windows/Linux 分支未执行；macOS 缺桌面操作 |

## 本轮行为清单

现有 `domain_behavior` 保护了局部 heal 计算，`product_behavior` 保护了
部分 stack 计算；它们没有断言下列 HTTP 请求、文件交付和后续操作。
函数名仅定位实现，测试不直接调用私有函数。

| 状态 | 入口及风险 | 前提、操作和公开断言 | 定向反例 |
| --- | --- | --- | --- |
| [x] | `/api/xmp` → `read_json`；超大声明导致不必要读入 | 声明大于 256 MiB，不发送正文；应立即返回 400，已保存编辑不变 | 去掉声明长度拒绝 |
| [x] | `/api/upload` → `api_upload`；超大上传占用内存 | 声明大于 500 MiB，不发送正文；应立即返回 413，图库不变 | 去掉上传声明长度拒绝 |
| [x] | `/api/xmp`；合法边界被误拒绝 | 流式发送恰好 256 MiB 的合法 JSON；保存成功且可重开 | 将上限比较改为包含等号 |
| [x] | `/api/xmp`；无长度声明绕过上限 | chunked 发送 256 MiB + 1 的合法 JSON；400，旧编辑不变 | 去掉实际接收长度检查 |
| [x] | `/api/heal` → `retouch::heal`；改错像素或无效修复 | 合成两颗亮点，只涂一颗；修复涂选区，逐像素核对未涂选区的 sRGB 结果，原照片字节不变 | 交付原图；误滤波整图；覆盖原照片 |
| [x] | `/api/heal` 的 master 连续性；丢失上次修复 | 第二次修复携带第一次返回的 master；最终文件保留两次修复 | 第二次忽略 session master |
| [x] | `/api/xmp` → `/api/develop`；保存重开丢失像素 | 中性 recipe 保存已修复 master，重启服务后预览仍有修复 | 将带 master 的中性保存当成 clear |
| [x] | `session_master`；将甲照片结果用于乙照片 | 甲照片合法取得 master，乙照片携带它请求 heal；400 | 接受其他照片签发的 master |
| [x] | `/api/heal` 部分失败；静默丢掉手工选择 | 显式指定不可用的外部检测运行时；手绘区域仍修复，响应披露检测失败 | 将可恢复检测失败改为全失败；丢掉披露 |
| [x] | `/api/stack` → `stack_files`；只返回某一输入或错误精度 | 两张已对齐的不同亮度合成图做 noise stack；像素来自两帧，尺寸和 16-bit 交付保留，源图不变 | 返回第一帧；量化为 8-bit；交付缩略尺寸 |
| [x] | `/api/stack` 的 master 连续性；合并丢失修复 | 第一帧已有 heal master；stack 结果保留该修复 | 第一帧忽略 master |
| [x] | `/api/stack` 错误路径；缩放不一致画幅并冒充成功 | 不同尺寸输入应拒绝并说明画幅不一致，输入不变 | 静默缩放后合并 |
| [x] | `/api/heal` 的失败后容量；空占位耗尽输出名额 | 999 次无有效选择的失败后，有效手绘修复仍能交付 | 失败后不释放空输出占位 |

## 隔离与运行

沿用真实服务子进程、随机端口、每例独立 cwd/HOME/数据目录。
图片与蒙版均合成。外部检测失败使用现有运行时配置指向不存在的程序，
且提供仅含报错的本地脚本；不请求模型，不引入生产专用分支。

普通用例进入 `cargo test --test serve_behavior`。
真实大输入与容量恢复用例显式 `--ignored`，逐项执行并记录资源原因；
速度只决定运行方式，不决定行为是否进入范围。

统一资源入口为 `scripts/http_resource_battery.sh`，已接入 build CI 的测试 job。
本地 Agent 可用 `AUTOSHADE_HTTP_RESOURCE_TESTS=1 scripts/agent_quality_gate.sh` 启用。
该开关只选择测试运行，不改变生产行为或上限；mutation 下仍须显式运行所选慢用例，
不能用基线单独通过代替故障版本的执行。

## 执行证据

本机证据目录：`/private/tmp/autoshade-master-audit.7BstC2`，不作为测试依赖。

- `tests/http_limits/mod.rs` / `tests/http_workflows/mod.rs` 新增 13 条；
  HTTP 测试合计 29 条默认、3 条资源 gate。基线和恢复后的两组均实际执行通过。
- `run_mutations.py` 在隔离副本逐个修改生产源码，`results.json` 记录
  18 个反例对应的唯一 case、退出码和结果；全部因对应断言失败，无编译失败或
  工具 timeout 计作 caught。每项有同名 `.patch` / `.log`。
- `baseline.log` / `restored.log` 与 `resources-baseline.log` /
  `resources-restored.log` 分别保存普通及资源套件的前后基线。
- `release-verified.log`：最终默认回归、clippy、架构、文本、字体和文档检查通过；
  外部 XMP census 因无语料明确未执行。`resource-final.log` 保存资源入口的
  2 + 1 条实际通过结果。CI 仅已配置，未声称在远端或其他平台执行过。
- `production-before.json` / `production-after.json`：269 个生产 Rust 文件
  校验值一致。本轮只补测试、测试执行入口及验收材料。
- `json-declared-length` / `upload-declared-length`：错误请求得到 100 Continue，
  而不是 400 / 413；`json-limit-off-by-one` 错误拒绝合法边界；
  `json-chunked-limit` 错误接受超限写入。
- `heal-delivers-original` / `heal-forgets-master` / `neutral-save-clears-master`
  分别因修复点仍然明亮或重开后修复消失而失败；
  `heal-overwrites-source` 因原文件字节变化而失败。
- `master-owner-bypass` 将其他照片的 master 错误接受为 200；
  `detector-error-discards-paint` 错误拒绝手工修复；
  `detector-error-silenced` 没有提供应有的部分失败披露。
- `stack-first-frame-only` 交付第一帧亮度，`stack-eight-bit` 降为 8-bit，
  `stack-preview-sized` 交付 16×12 而非 32×24，
  `stack-forgets-heal` 丢失第一帧修复，`stack-resizes-mismatch` 错误接受不一致画幅。
- `failed-heal-leaks-claim` 最终响应
  `no free heal output name (999 in the masters folder)`，不是无法归因的通用 500。

### 对新用例的反向复核

最初“只修改涂选区域”的用例只检查一个未涂选点。
`probe_unpainted.py` 注入整图滤波时，`unpainted-before.log` 仍为通过。
这说明原断言不足，不能以当时已有反例被捕获就宣称该行为完整受保护。

补强后，将源图和交付图归一到 sRGB，遍历输入蒙版的全部未涂选像素，
保留一个 8-bit 量化级的容差。相同反例在 `heal-filters-unpainted.log` 失败：
未涂选像素 `(0, 0)` 从 80 变成 0。未增加 case 数量，也不依据覆盖率作判断。

## 服务子进程的覆盖率诊断

当前验证环境为 macOS aarch64、Rust 1.94.1 / LLVM 21.1.8，
`cargo-llvm-cov 0.9.1`，系统页大小 16384 字节。
只增加 `--runtime-counter-relocation` 仍报计数器未对齐；
只对齐 counter / data 后改为 bitmap 未对齐。
将三个 profile 段都按本机页大小对齐后，原有单个导出用例正常运行，
不再出现 LLVM profile 错误，报告出现该服务子进程的真实执行记录。这个结果只证明
该一个探针，不是全套测试的通用配置。

可复现的本机诊断命令如下；参数只作用于此次诊断构建，不改 Cargo 配置或生产源码：

```bash
env \
  RUSTFLAGS='-C link-arg=-Wl,-sectalign,__DATA,__llvm_prf_cnts,0x4000 -C link-arg=-Wl,-sectalign,__DATA,__llvm_prf_data,0x4000 -C link-arg=-Wl,-sectalign,__DATA,__llvm_prf_bits,0x4000' \
  LLVM_PROFILE_FILE_NAME='http-diagnostic-%p%c.profraw' \
  cargo llvm-cov --locked --test serve_behavior --json \
  --output-path target/http-diagnostic.json
```

有原有 `RUSTFLAGS` 时应保留相应参数，不把此命令当作通用替换。
其它系统、页大小及工具链需要重新验证，不能无条件照搬。
`coverage-mapped-probe.json` 是同一导出 case 的对照；
`http-diagnostic.log` / `http-diagnostic.json` 是 HTTP 默认套件的诊断。
资源慢测试没有包含在该统计中，也不与此前全仓百分比作直接比较。
修正测量不等于增强测试，行为保护仍由上述断言与反例证明。

### 本轮扩大复核的失败

本轮用同样的三段页对齐参数运行默认 `--all-targets`，并显式跳过已知红灯的并发
填充。执行到 `judge_behavior` 时，插桩测试二进制因 SIGSEGV 退出，没有生成报告。
随后用同一构建补跑此前未到达的测试目标，`cli_behavior` 同样在运行测试前 SIGSEGV。

因此：

- 没有可用的本轮全仓覆盖率数值，也不沿用旧数值冒充当前结果。
- 已完成的正常 release 基线与 mutation 证据不受该插桩崩溃影响。
- 三段页对齐只保留为窄探针记录；在解释 SIGSEGV 前，不进入默认质量门禁。
- 覆盖率仍只用于诊断，不为了取得报告修改生产代码或选择测试。

## 仍需推进

- 本页的 GUI 启动、偏好和对应平台验收仍未执行。
- `api_retouch` 的生成式填充链、生成像素 provenance、模型等待期间的其他操作，
  不能由本页不调用模型的 heal/noise stack 用例代替。
  后续计划与证据见 `GENERATIVE_WORKFLOW_AUDIT.md`。
- aggregate body budget、完整 RAW/full-resolution 资源预算需要独立审计。
  本页只证明请求体大小政策和失败后输出容量，未证明进程峰值内存上限。
- 本页只覆盖 noise stack 的这些交付契约，不将它外推成 HDR/fuse/focus 全部验证。
