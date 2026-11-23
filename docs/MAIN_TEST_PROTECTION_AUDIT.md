# Main 分支测试保护审计

审计基线：

- 来源：`main@dce7e8a3594850bdaf6d05b2f50eb60dc6804414`
- 当前工作树：`refactor/typed-routing`，包含尚未提交的行为测试
- 日期：2026-11-23

本审计把 `main` 当作行为事实来源，不把旧测试代码当作可直接复制的资产。每个旧
case 最终必须归入以下一种状态：

- `equivalent`：当前已有公开行为测试逐条执行了旧断言所表达的产品契约。
- `rewritten`：旧行为有价值，但已改写为公开 API、文件结果、错误或可见 UI 测试。
- `lint`：旧断言只约束依赖、模块边界、禁用 API 或代码形状，由编译器、clippy
  或架构 gate 接管。
- `drop`：旧断言只锁定私有字段、helper、调用顺序、源码文本或重构中间态，没有
  独立产品故障语义。
- `external-gate`：行为有价值，但依赖付费、真实语料、平台或长时间资源，保留明确
  启用方式。

`equivalent` 不能只靠相似的测试名认定。旧 case 中每条断言必须能指出当前执行者；
任一断言没有执行者时，只能标为 `rewritten` 待完成或 `drop` 并说明为什么不是产品
契约。

## Inventory

解析全部 Rust `#[test]` 后：

- `main`：2,281 cases，约 11,399 个断言宏，445 个显式或 feature-gated case。
- 当前工作树：485 cases，约 1,219 个断言宏，31 个显式 ignore。
- 名称完全相同的 case 只有 1 个；不能按名称自动宣称其余 case 已覆盖。

完整逐 case 数据位于 `docs/main_test_case_audit.tsv`。默认运行
`python3 scripts/check_main_test_audit.py` 会拒绝漏项、重复项、无证据裁决和任何
`open`；目标推进期间可用 `--allow-open` 查看真实剩余量，但该模式不能作为完成门。
当前校验结果：2,281 行与 main snapshot 精确一致，71 条 `rewritten`、11 条
`lint`、2,199 条 `open`。

| 家族 | main cases | main 断言宏 | gated | 状态 |
| --- | ---: | ---: | ---: | --- |
| GUI | 444 | 2,538 | 1 | 审计中 |
| Render | 350 | 1,403 | 1 | 待审计 |
| XMP | 242 | 1,479 | 0 | 待审计 |
| Fit Zoned | 241 | 1,168 | 240 | 审计中 |
| Fit | 169 | 845 | 169 | 审计中 |
| Store | 103 | 545 | 0 | 待审计 |
| Pipeline | 98 | 532 | 0 | 待审计 |
| Recipe | 50 | 322 | 0 | 待审计 |
| Denoise | 48 | 167 | 0 | 待审计 |
| Advisor | 39 | 222 | 0 | 待审计 |
| Decode | 37 | 163 | 0 | 待审计 |
| Segment | 34 | 150 | 34 | 待审计 |
| Config | 32 | 157 | 0 | 待审计 |
| CLI main | 29 | 146 | 0 | 待审计 |
| Serve | 27 | 162 | 0 | 待审计 |
| Generative | 22 | 93 | 0 | 待审计 |
| Stack | 22 | 72 | 0 | 待审计 |
| Embed | 21 | 99 | 0 | 待审计 |
| LCP | 21 | 73 | 0 | 待审计 |
| Eval | 20 | 123 | 0 | 待审计 |
| Fit Field | 20 | 79 | 0 | 待审计 |
| 其余 27 个小家族 | 182 | 766 | 0 | 待审计 |

这里的数量仅用于证明 inventory 完整，不是质量指标，也不是迁移目标。

### 架构断言的首批归属

11 个 main case 已在逐 case TSV 中归为 `lint`，执行者分别是
`check_architecture.py` 的 RAW plane/develop ceiling、Store capped read、
engine/analyzer boundary、base-look single entry、paint notification、server/generative/heal
slot、linear-ramp single definition 与 `gui-logic-headless` 规则。

`src/bin/gui/tests/architecture.rs::the_migrated_panels_stay_behind_the_seam` 没有被伪标
完成：当前 `panels/` 仍存在直接写 dirty/toast/store 和 `start_*` 的生产路径。它是
架构事实而非行为测试，但新增等价 lint 会在当前代码上失败；修复需要生产重构，与
本 goal 的“不得为测试修改生产代码”冲突，因此保持 `open`，而不是复制源码扫描或
静默删除。

## 已裁决

### Fit：identity pair

- Main case：`src/fit/tests.rs::identity_fit_is_near_neutral`
- 当前 case：`tests/fit_workflow.rs::an_identity_pair_delivers_a_neutral_fit_without_a_terminal_reset`
- 状态：`rewritten`
- Gate：`scripts/fit_battery.sh`

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| rationale 不含 `do-no-harm terminal case` | 同一公开 rationale 断言 | 保留；防止坏解被终端重置伪装成成功 |
| exposure 接近 0 | 实际交付预览与输入的平均绝对像素差 `< 0.5` | 改写；不锁具体 slider 分解 |
| contrast/highlights/shadows/whites/blacks/saturation 各自接近 0 | 同一最终像素断言 | 改写；用户契约是 identity look 不变，不是 solver 必须选择某组内部参数 |
| `err_after < 0.02` | 实际调用 `render::develop_preview` 后比较交付像素 | 改写；真实交付优先于 solver 自报指标 |

Mutation 证据：临时让公开 `fit_recipe` 返回前强制 `exposure_ev = 1.0`，当前 case 在
“identity fit changed the delivered pixels”断言处失败；恢复后通过。最终生产 Rust 无
该 mutation。

### Fit：engine-render round trip

- Main case：`src/fit/tests.rs::roundtrip_recovers_tone_and_saturation`
- 当前 case：`tests/fit_workflow.rs::fitting_an_engine_render_reproduces_its_delivered_look`
- 状态：`rewritten`
- Gate：`scripts/fit_battery.sh`

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| exposure 接近 `0.35` | 无直接 slider 断言 | 删除实现绑定；多个等价 recipe 可以交付相同 look |
| contrast 位于 `3..45` | 无直接 slider 断言 | 同上 |
| highlights 位于 `-50..-8` | 无直接 slider 断言 | 同上 |
| saturation 位于 `5..30` | 无直接 slider 断言 | 同上 |
| `err_after` 至少减半 | 对真实 fitted preview 与 target 的平均绝对像素误差至少减半 | 改写为实际交付结果 |

Mutation 证据：临时让公开 `fit_recipe` 丢弃求解结果并返回 `EditRecipe::default()`，
当前 case 在 `before=8.799 after=8.799` 的交付像素断言处失败；恢复后通过。最终生产
Rust 无该 mutation。

### Fit：haze 可见结果

- Main case：`src/fit/tests.rs::the_haze_pairs_broken_identification_falls_back_and_still_fits`
- 当前 case：`tests/fit_workflow.rs::fitting_a_hazy_render_removes_most_of_the_visible_gap`
- 状态：`rewritten`
- Gate：`scripts/fit_battery.sh`

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| 私有 `neutral_gate_misprediction` 超过内部阈值 | 无直接断言 | `drop`；它只指定当前识别算法，不能独立说明产品坏了 |
| `report.err_after < 0.35 * report.err_before` | 对真实 fitted preview 与 clean target 的平均绝对像素差 `< 0.35 * before` | 改写；验收真实交付而非 solver 自报指标 |

Mutation 证据：临时让公开 `fit_recipe` 丢弃求解 recipe，当前 case 在
`before=25.248 after=25.248` 的交付断言处失败；恢复后通过。

`src/fit/tests.rs::hazy_to_clean_fit_stays_sane` 尚未完全裁决：其“拟合不得恶化”的断言
已由当前 case 执行；具体 slider 不得接近极值和逐 hue-band 的最终颜色约束仍需分别
判断是公开可编辑性契约还是内部参数偏好，不能用本 case 提前勾销。

### Fit：局部暖化不得污染未变天空

- Main case：`src/fit/tests.rs::warm_rock_cast_must_not_violet_the_pale_sky`
- 当前 case：`tests/fit_workflow.rs::fitting_warm_rocks_does_not_turn_the_unchanged_sky_purple`
- 状态：`rewritten`
- Gate：`scripts/fit_battery.sh`

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| 天空区域必须含可测色像素 | 测试 oracle `mean_hue` 明确拒绝无色 premise | 保留 premise |
| 交付天空平均 hue 与目标天空相差 `< 30°` | 同一最终像素 hue 断言 | 保留产品行为，不调用私有 hue/veto helper |
| Fit 不得使整幅结果更差 | 实际交付与 target 的像素误差不超过未编辑 source | 保留 |

Mutation 证据：临时向公开 `fit_recipe` 的返回 recipe 注入强红曲线，交付天空从
`213.9°` 移到 `348.7°`，当前 case 在 hue 断言处失败；恢复后通过。

`src/fit/tests.rs::foreign_hue_veto_measures_real_canyon_reconstruction` 中关于私有
`foreign_share`、`evidence_model` 和 veto helper 的断言归为 `drop`；其产品结果分别
由 haze 可见改进和 unchanged-sky case 执行。它关于 unsupported pixels 是否参与
内部 veto 的单独算法事实，仍需从公开 Fit 结果构造反例后才能认定已覆盖。

### Fit：光源变化使用可编辑 White Balance

- Main case：`src/fit/tests.rs::a_full_mode_pair_whose_light_changed_colour_gets_a_white_balance`
- 当前 case：`tests/fit_workflow.rs::fitting_an_illuminant_change_uses_white_balance_and_improves_the_delivery`
- 状态：`rewritten`
- Gate：`scripts/fit_battery.sh`

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| 同帧光源变化处于 Full mode / pixel pairing | 无直接 mode/pairing 断言 | `drop`；这是当前求解路线，不是用户结果 |
| 暖化目标得到 `temperature_k > 5500` | 同一公开 recipe 字段断言 | 保留；White Balance 是用户可编辑契约 |
| report 含 WB cells-vouched、且不含 cells-refused | 实际 fitted preview 必须比中性 preview 更接近目标 | 改写；先验 helper verdict 不替代交付结果 |

Mutation 证据：临时清空公开 `fit_recipe` 返回 recipe 的 `temperature_k`，当前 case
在“warmer illuminant change was not represented as editable white balance”断言处失败；
恢复后通过。

`a_full_mode_white_balance_ships_only_where_the_target_cells_vouch_it` 中 identity 不得
发明 illuminant 的公开语义尚未由本 case 勾销；其私有 cell-vouch 方向断言不直接
迁移。

### Fit：结构变化必须诚实披露

- Main cases：
  - `src/fit/tests.rs::content_divergence_fires_on_flat_sky_to_cloud_deck`
  - `src/fit/tests.rs::atmosphere_rationale_names_unrecoverable_structure_and_discloses_d`
- 当前 case：`tests/fit_workflow.rs::a_changed_sky_structure_is_disclosed_as_unrecoverable`
- 状态：`rewritten`
- Gate：`scripts/fit_battery.sh`

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| 私有 divergence reading 超过内部阈值 | `FitReport.mode == Atmosphere` | 改写为公开选择结果 |
| rationale 明说 structure cannot be reconstructed | 同一公开 rationale 断言 | 保留 |
| rationale 中内部 `D={value}` 与私有 reading 完全一致 | 无直接数值断言 | `drop`；内部诊断数值可重标定，诚实披露仍受保护 |
| Atmosphere confidence note 存在 | 暂无执行者 | 保留为后续 disclosure 审计，不能由当前 case 勾销 |

Mutation 证据：临时把公开 `fit_recipe` 返回的 mode 强制为 `Full`，当前 case 在
`Full != Atmosphere` 处失败；恢复后通过。

### Fit：不同画幅的参考图

- Main case：`src/fit/tests.rs::a_differently_shaped_reference_is_warned_about_not_refused`
- 当前 cases：
  - `tests/fit_workflow.rs::a_differently_shaped_reference_has_a_typed_disclosure`
  - `tests/fit_workflow.rs::a_differently_shaped_reference_returns_a_bounded_fit`
- 状态：`rewritten`
- Gate：`scripts/fit_battery.sh`

原 case 有“披露”和“仍返回结果但限制 confidence”两个主张，按一 case 一主张拆开。

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| 私有 `same_frame_plausible` 对 crop/resize 的真假 | 无直接 helper 断言 | `drop`；公开 Fit 结果决定用户行为 |
| typed `FIT_NOTE_NOT_SAME_FRAME` 存在 | `...has_a_typed_disclosure` | 保留；本地化视图读取 typed note |
| `err_after` 有限、仍返回 recipe | `...returns_a_bounded_fit` | 保留 |
| mismatched frame confidence 不超过 0.5 | `...returns_a_bounded_fit` | 保留公开可信度契约 |
| same-frame fixture 可高于 0.5 | 尚无执行者 | 不能由 mismatch case 勾销；留给 confidence 对照审计 |
| confidence 不低于内部 floor | 无直接断言 | `drop`；内部标定可调整，只保留“不伪报高置信且不拒绝” |

Mutation 证据：

- 删除返回 report 的 `FIT_NOTE_NOT_SAME_FRAME` typed note 时，disclosure case 失败。
- 强制把返回 confidence 写成 `0.9` 时，bounded-fit case 在高置信断言处失败。
- 两次 mutation 均已恢复，生产 Rust 无残留 diff。

### Fit：相反颜色族由分色控件交付

- Main case：`src/fit/tests.rs::per_band_colour_is_solved_from_two_sided_population_evidence`
- 当前 case：`tests/fit_workflow.rs::opposite_colour_family_changes_are_delivered_as_editable_band_controls`
- 状态：`rewritten`
- Gate：`scripts/fit_battery.sh`

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| fixture 走 Full mode | 无直接 mode 断言 | `drop`；用户契约是交付的 editable look |
| Green saturation 恢复负向、Blue 恢复正向且达到主要幅度 | 同一公开 HSL recipe 方向/幅度断言 | 保留 |
| global saturation 接近 0 | 无直接断言 | `drop`；若最终 look 和分色可编辑性正确，不限制等价的全局补偿 |
| report 自报 error 下降 | 对真实 delivered preview 与 target 的像素误差下降 | 改写 |
| typed note 枚举 Green/Blue 移动 | 暂无执行者 | 保留为后续 disclosure 审计，不能由当前 case 勾销 |

Mutation 证据：临时把公开 `fit_recipe` 返回 recipe 的 HSL 全部清零，当前 case 在
Green/Blue 分色断言处失败；恢复后通过。

下列 main case 与它有关但尚未被提前勾销：

- `a_one_sided_band_is_refused_by_name_never_read_as_equal`：需要公开的单边 band 拒绝与
  typed disclosure。
- `a_band_rotated_a_few_degrees_is_solved_back_to_that_band_and_no_other`：Hue 轴的独立
  inverse problem。
- `a_band_whose_ranges_ask_opposite_rotations_ships_nothing`：同一 band 内相反请求的拒绝。
- `hsl_band_budget_is_monotone_across_the_three_strength_stops`：强度 dial 对交付幅度的
  实际影响，不能只测 `FitBudget` 数值。

### Fit Zoned / XMP：zone 跨 sidecar 的交付一致性

- Main case：`src/fit_zoned/tests.rs::a_zone_survives_the_sidecar_round_trip_byte_for_byte`
- 当前 case：`tests/fit_workflow.rs::a_fitted_zone_survives_xmp_as_the_same_delivered_pixels`
- 状态：`rewritten`
- Gate：`scripts/fit_battery.sh`（Unix）

当前 case 通过公开 `fit_recipe_zoned → recipe_to_xmp → xmp_to_recipe →
develop_preview`。测试只补回产品契约明确要求本机重新生成的 AI raster；不调用
main 的私有 `attach_zones`、`ai_mask_xml` 或 `mask_weight` helper。

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| zoned fit 确实产生 Select Sky carrier | 从公开 report 选取 `ZoneSky` AI mask | 保留 premise |
| 反转 Land zone 的标准 XMP bit 写为 net inversion | 公开 XMP 文本含 `Mask/Image` 与 `crs:MaskInverted="true"` | 保留 Lightroom 互操作文件契约 |
| AI raster 在自家读取后需要重新生成/补回 | 读取后显式补回同一 raster | 保留公开设计，不把缺失 raster 当失败 |
| sidecar 前后 render 字节一致 | `delivered == expected` | 保留最终产品结果 |
| main 中关于具体内部 geometry/adjustment 两位各放在哪里的断言 | 无直接私有字段断言 | 由公开 XMP spelling 与最终像素共同替代 |

Mutation 过程也验证了 fixture：

1. 初版 fixture 没有固定 geometry-owned bit，同一 writer mutation 仍通过，因此不计
   保护。
2. 将 carrier 归一为 geometry 不反转、adjustment 反转后，基线通过。
3. 临时把 `lr_net_inverted` 改成只写 geometry bit 时，case 在公开 XMP
   `MaskInverted` 断言处失败；恢复后通过。

## Render 已裁决

以下 case 全部从公开 `render::develop_preview` 进入，不复制 main 的
`apply_develop_anon`、`apply_dehaze`、`apply_hsl`、LUT 或 mask-weight helper。

### Neutral develop

- Main case：`src/render/tests/dehaze_texture_clarity.rs::neutral_recipe_is_near_identity`
- 当前 case：`tests/render_behavior.rs::a_neutral_recipe_preserves_the_delivered_pixels`
- 状态：`rewritten`
- 映射：main 对两个 float 像素逐通道容差 `< 0.02`；当前改为合成 32×24 RGB
  图像，公开预览交付逐通道最大偏移不超过一个 8-bit code。
- Mutation：公开 preview 入口额外加 `+1 EV` 时，当前 case 以最大偏移 78 codes
  失败；恢复后通过。

### Global Dehaze

- Main case：`src/render/tests/dehaze_texture_clarity.rs::dehaze_positive_recovers_a_hazy_ramp`
- 当前 case：`tests/render_behavior.rs::positive_dehaze_restores_tonal_span_and_colour`
- 状态：`rewritten`
- 映射：保留线性光散射 fixture，以及“q90-q10 明暗跨度至少提高 15%”和“平均
  chroma 至少提高 10%”两个共同描述 haze recovery 的结果；不调用私有 dehaze
  算子。
- Fixture 纠正：初版在 gamma 空间混合 haze，premise 不成立并红灯；改为
  `I = J*t + A*(1-t)` 的线性光 fixture 后基线通过，没有放宽阈值。

### Local Dehaze

- Main case：`src/render/tests/local_effects_and_hsl.rs::mask_dehaze_renders_only_inside_the_mask`
- 当前 case：`tests/render_behavior.rs::local_dehaze_leaves_uncovered_pixels_unchanged`
- 状态：`rewritten`
- 映射：保留“全覆盖区域确实改变”和“零覆盖半幅逐像素不变”；删除对内部
  smoothstep 的具体列号解释。

### HSL routing

- Main case：`src/render/tests/local_effects_and_hsl.rs::hsl_adjusts_only_the_targeted_colour_band`
- 当前 case：`tests/render_behavior.rs::hsl_saturation_changes_only_the_target_colour_family`
- 状态：`rewritten`
- 映射：Red band 去饱和后 channel spread `< 16` codes；Blue family 三通道相对
  neutral render 偏移均不超过 2 codes。main 对 Yellow/Green/Aqua/Purple 的扩展
  census 尚未由此 case 勾销，留给色族矩阵审计。

### Local Temperature

- Main case：`src/render/tests/local_wb_and_colour_management.rs::local_temperature_warms_the_masked_region_only`
- 当前 case：`tests/render_behavior.rs::local_temperature_warms_only_the_covered_region`
- 状态：`rewritten`
- 映射：gradient 零端等于 neutral render；全覆盖端 red 上升且 blue 下降。main
  对四行精确 coverage 权重的断言归入 linear-mask coverage 审计，不混入 WB case。

### Sharpening

- Main case：`src/render/tests/dehaze_texture_clarity.rs::sharpening_raises_local_contrast_at_an_edge`
- 当前 case：`tests/render_behavior.rs::sharpening_strengthens_an_edge_without_moving_far_flat_pixels`
- 状态：`rewritten`
- 映射：真实预览的边缘 step 必须变强，Gaussian 支持范围外的平坦像素逐字节不变。
  main 的 float 通道灰度相等断言尚未由此 case 勾销，留给 detail colour-neutral
  审计。

### RGB curves

- Main case：`src/render/tests/local_effects_and_hsl.rs::rgb_curves_shape_each_channel_independently`
- 当前 case：`tests/render_behavior.rs::each_rgb_curve_controls_only_its_own_channel`
- 状态：`rewritten`
- 映射：Red/Green/Blue 三条公开 recipe 曲线逐一运行；目标通道从黑场提升超过 38
  codes，其余通道保持低于 5 codes。

共同 mutation 证据：临时让公开 `develop_preview` 原样返回输入后，上述六个非中性
case 分别在自身的 Dehaze、局部覆盖、HSL、Temperature、Sharpening 和 RGB curve
断言处失败；neutral case 按 premise 继续通过。结合 neutral 的 `+1 EV` mutation，
七个新增不变量都有对应红灯。生产 `src/render/develop.rs` 已恢复、无最终 diff。

### Tone ramp safety

- Main cases：
  - `src/render/tests/tone_and_colour.rs::region_tones_pin_endpoints_and_stay_monotonic`
  - `src/render/tests/tone_and_colour.rs::no_slider_setting_collapses_a_tonal_band`
  - `src/render/tests/tone_and_colour.rs::no_slider_collapses_an_interior_band_at_any_exposure`
- 当前 case：`tests/render_behavior.rs::extreme_tone_controls_keep_a_monotonic_tonal_ramp`
- 状态：`rewritten`（当前覆盖 tone region controls；不同 exposure 的额外抽样仍待审）

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| extreme tone controls 下 LUT 不下降 | 256-code 灰阶通过公开 preview 后逐像素不下降 | 改写为交付像素 |
| 黑白端点与各 region 内部细分 | 至少保留 32 个交付灰阶 | 合并为“不能折叠 tonal ramp”的公开结果 |
| 任意 exposure 下的内部 band 分离 | 暂无完整矩阵 | 不能由当前 case 勾销 |

Mutation 证据：把 tone LUT 的单个中点清零没有稳定到达 8-bit 交付，不能计为保护；
改为将整条 tone LUT 反向后，当前 case 在第一个倒序像素处失败。生产
`src/render/tone.rs` 已恢复。

### Shadow Color Grade

- Main case：`src/render/tests/local_effects_and_hsl.rs::color_grade_tints_the_targeted_tonal_region`
- 当前 case：`tests/render_behavior.rs::shadow_colour_grading_leaves_highlights_neutral`
- 状态：`rewritten`
- 映射：Shadow wheel 使暗灰 blue 通道占优；亮灰相对 neutral render 每通道偏移
  不超过 2 codes。main 的 neutral grade no-op 已由全入口 bypass 对照和 neutral
  render case共同覆盖。

### Point Color

- Main case：`src/render/tests/tone_and_colour.rs::a_point_color_moves_its_own_colour_and_nothing_else`
- 当前 case：`tests/render_behavior.rs::point_colour_moves_its_sampled_colour_without_moving_a_distant_hue`
- 状态：`rewritten`
- 映射：使用公开 `point_color_at` 建 swatch；采样 Red 的 chroma spread 降到
  32 codes 以下，远端 Blue 三通道相对 neutral render 均不超过 2 codes。

### Local Point Curve

- Main case：`src/render/tests/local_effects_and_hsl.rs::a_local_point_curve_darkens_only_inside_the_mask`
- 当前 case：`tests/render_behavior.rs::a_local_point_curve_leaves_uncovered_pixels_unchanged`
- 状态：`rewritten`
- 映射：公开 recipe 的 covered edge 至少降低 10 codes；零覆盖半幅逐像素等于
  neutral render。main 的 local Red/Green/Blue curve 三通道路由仍由全局 RGB curve
  case 部分覆盖，局部 per-channel 路由尚未单独勾销。

共同 mutation 证据：公开 preview 入口原样返回输入时，Color Grade、Point Color 和
Local Point Curve 三个 case 各自在自己的可见像素断言处失败；恢复后通过。

### Linear mask transition

- Main cases：
  - `src/render/tests/local_wb_and_colour_management.rs::linear_mask_affects_only_the_full_end`
  - `src/render/tests/coverage_and_ramps.rs::axis_aligned_linear_coverage_is_byte_stable`
- 当前 case：`tests/render_behavior.rs::a_linear_mask_delivers_a_smooth_monotonic_transition`
- 状态：`rewritten`
- 映射：8 行灰阶经公开 preview 后必须逐行严格变暗，证明 linear mask 是连续过渡而
  不是“任意正权重即全覆盖”的硬 step。main 对每个内部 coverage 数值的精确钉值归入
  Lightroom parity / profile 审计，不由当前 case 勾销。

### Bitmap alpha

- Main case：`src/render/tests/tone_and_colour.rs::bitmap_mask_sampling_matches_the_producers_convention`
- 当前 case：`tests/render_behavior.rs::a_bitmap_mask_scales_the_effect_with_its_stored_alpha`
- 状态：`rewritten`
- 映射：真实 8-bit mask 文件从 0 到 255，公开 preview 的曝光增益从 0 开始并逐像素
  严格增加；不直接调用私有 bitmap sampler。

Linear 与 Bitmap 的 mutation 证据：公开 preview 入口原样返回输入时，两条 case
分别得到常量 153 和全零 gain，并在自己的渐变断言处失败。

### Missing inverted bitmap

- Main case：`src/render/tests/radial_and_bitmap_masks.rs::missing_bitmap_raster_is_inert_even_when_inverted`
- 当前 case：`tests/render_behavior.rs::a_missing_inverted_bitmap_mask_is_inert`
- 状态：`rewritten`
- 映射：不存在的 bitmap path、adjustment inverted、`+2 EV`；公开交付必须逐像素
  等于 neutral render，不能把“无 alpha”反成整幅覆盖。
- Mutation：临时删除 `Cover::new` 对缺失 raster 的提前拒绝，交付从 128 变为整幅
  238，case 失败；恢复后通过。

### Radial feather

- Main cases：
  - `src/render/tests/radial_and_bitmap_masks.rs::radial_feather_zero_stays_finite_on_the_boundary`
  - `src/render/tests/coverage_and_ramps.rs` 的 radial falloff 系列
- 当前 case：`tests/render_behavior.rs::radial_feather_produces_intermediate_coverage`
- 状态：`rewritten`（feather=75 的公开行为；main 的 Lightroom 全标定矩阵仍待外部
  parity gate 裁决）
- 映射：公开 `mask_coverage` 必须同时包含 0、255 和至少一个 1..254 中间值。
- Mutation：强制调用 `radial_falloff(0, d)` 后 coverage 只有硬边，case 在中间覆盖
  断言处失败；恢复后通过。

### Export 遇到丢失的活动蒙版

- Main case：`src/render/tests/geometry_and_lens.rs::export_refuses_a_recipe_whose_mask_raster_is_unreadable`
- 当前 case：`tests/render_behavior.rs::export_with_an_unreadable_active_mask_writes_nothing_and_names_the_mask`
- 状态：`rewritten`
- 映射：真实 source PNG、活动 `sky` bitmap adjustment、缺失 raster；公开
  `render_to_file` 必须返回包含 mask 名称的错误，且目标文件不存在。main 的 amount=0
  和 parked-mask 对照尚未由此 case 勾销。
- Mutation：把 baked export 的严格 `load_mask_raster_snapshot` 换成 best-effort
  loader 后，API 错误变为 `Ok((16, 12))`，case 在 `unwrap_err` 处失败；恢复后通过。

### Profile vignetting

- Main case：`src/render/tests/geometry_and_lens.rs::lens_profile_vignette_lifts_corners_only_and_geometry_roundtrips`
- 当前 case：`tests/render_behavior.rs::profile_vignetting_lifts_corners_without_moving_the_center`
- 状态：`rewritten`（vignetting arm）
- 映射：真实公开 preview 上中心保持 code 100、角落提升到大于 110；main 同 case 的
  distortion/CA roundtrip 部分由现有 geometry properties 与下方 manual CA case分别
  执行。
- Mutation：preview 入口原样返回输入时，角落仍为 100，case 失败。

### Manual chromatic aberration

- Main case：`src/render/tests/raw_matrix_and_ca.rs::manual_ca_shifts_the_red_channel_radius`
- 当前 case：`tests/render_behavior.rs::manual_chromatic_aberration_moves_only_the_red_channel`
- 状态：`rewritten`
- 映射：公开 `geometry_profile + apply_lens_geometry` 处理同通道同心环；Red 必须有像素
  变化，Green/Blue 必须逐像素不变。main 的具体 sub-pixel 坐标和内部常量不迁移。
- Mutation：强制关闭 `apply_lens_geometry` 的 CA arm 后，case 在“manual CA did not
  move red”处失败；恢复后通过。

### Straighten direction

- Main case：`src/render/tests/geometry_and_lens.rs::straighten_rotation_geometry_and_direction`
- 当前 case：`tests/render_behavior.rs::positive_straighten_rotates_the_frame_clockwise`
- 状态：`rewritten`（方向主张）
- 映射：Red/Blue 垂直分割图经过公开 `rotate_straighten(+10°)`，顶部中心右侧必须落在
  Red；main 同 case 的 0° identity、inscribed dimensions 和无黑角断言由后续几何
  审计分别归属，不能由方向 case 一并勾销。
- Mutation：把传入角度乘 0 后，探针保持 Blue，方向 case 失败；恢复后通过。

## Store 已裁决

### Lightroom newest intent

- Main case：`src/store/tests.rs::lightroom_sidecar_newest_intent_wins`
- 当前 case：`tests/store_behavior.rs::the_newest_lightroom_sidecar_intent_wins_over_the_store`
- 状态：`rewritten`
- 映射：显式设置文件 mtime，不使用 sleep；较旧 Lightroom sidecar 返回
  `OlderThanStore`，较新 sidecar 返回 `NewerThanStore` 且携带实际读取文本。main 的
  “无 sidecar / sidecar-only / 与自家 projection 字节相同”三个独立分支仍由后续
  ranking 审计分别归属。
- Mutation：反转 sidecar/store 的 mtime 比较后，older 对照先错误返回 newer，case
  失败；恢复后通过。

### Settings lock

- Main case：`src/store/tests.rs::a_settings_lock_serializes_writers_and_reenters_on_its_thread`
- 当前 cases：
  - `tests/store_behavior.rs::the_settings_lock_reenters_on_its_own_thread`
  - `tests/store_behavior.rs::the_settings_lock_refuses_other_threads_until_the_owner_releases_it`
- 状态：`rewritten`

原 case 的两个主张按一 case 一主张拆开：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| 同线程复合 load/write 可重入 | reentrant case 返回嵌套值 7 | 保留 |
| 其他线程 NoWait 得到 `WouldBlock` | contention case | 保留 |
| owner drop 后可再次获得锁 | contention case 最后一次公开调用 | 保留 |

Mutation 证据：

- 让公开 `with_settings_lock` 直接执行闭包时，other-thread case 在 `WouldBlock`
  断言处失败。
- 关闭 `with_path_lock` 的同线程 held-set 检查时，nested NoWait 返回
  `WouldBlock`，reentrant case 失败。
- 两次 mutation 均已恢复。

### 暂不按旧 fixture 迁移的崩溃恢复

以下 main case 直接写内部事务 marker；按新原则不能复制：

- `a_killed_version_delete_resumes_and_its_number_stays_burned`
- `a_pending_clear_completes_on_the_next_touch`

两项行为仍有产品价值，状态为 `rewritten` 待完成。验收必须通过真实子进程在公开
`delete_version` / `clear_develop` 中途被终止，再从公开 list/read/claim 入口验证
恢复；不能直接创建 `.deleting.vN`、`clear.pending` 或断言内部 registry 文件布局。

## XMP 已裁决

### Fresh document carries the complete develop

- Main case：`src/xmp/payload_tests.rs::a_fresh_document_carries_the_whole_develop_and_reads_it_back_exactly`
- 当前 case：`tests/xmp_behavior.rs::a_fresh_xmp_document_round_trips_the_complete_develop`
- 状态：`rewritten`
- 映射：公开 recipe 同时带 global controls、as-shot anchor、colour field、disabled
  Zone mask、Intersect component、color gains、rationale/confidence；`recipe_to_xmp →
  xmp_to_recipe` 后完整 `EditRecipe` 相等。当前 fixture 不带 raster 文件；main 关于
  raster embedding/relocation 的断言仍单独审计。
- Mutation：关闭 fresh writer 的 payload embedding 后，CRS-only 读回丢失
  as-shot anchor、colour field、mask 私有状态且 exposure 被量化，完整相等断言失败；
  恢复后通过。

### Corrupt payload falls back with disclosure

- Main case：`src/xmp/payload_tests.rs::a_payload_this_build_cannot_trust_is_disclosed_and_the_crs_reading_stands`
- 当前 case：`tests/xmp_behavior.rs::a_corrupt_payload_is_disclosed_and_the_crs_projection_still_opens`
- 状态：`rewritten`
- 映射：从公开 writer 生成 document 后篡改 payload；公开 diagnostic reader 必须
  保留 CRS exposure、拒绝 payload-only colour field，并向注入 `Collector` 发出
  payload/checksum 披露。
- Mutation：让 `xmp_to_recipe_with_diag` 改走 silent reader 后，recipe fallback 仍
  正确但 diagnostics 为空，case 在披露断言处失败；恢复后通过。

### Merge around a foreign namespace prefix

- Main case：`src/xmp/payload_tests.rs::a_merge_replaces_the_previous_payload_and_steps_around_a_foreign_prefix`
- 当前 case：`tests/xmp_behavior.rs::merging_around_a_foreign_prefix_preserves_both_namespaces`
- 状态：`rewritten`（foreign-prefix arm）
- 映射：输入文档已把 `asr` 绑定给外部 URI；公开 merge 必须保留外来绑定和属性，
  以 `asr1` 写入 AutoShade payload，读回完整 recipe。main 的“再次 merge 替换旧
  asr1 payload”仍待幂等 merge 审计。
- Mutation：让公开 merge wrapper 直接返回 `None`，case 在合法文档被拒绝处失败；
  恢复后通过。

## GUI 已裁决

### 拖放照片进入编辑画布

- Main 保护形态：`src/bin/gui/tests.rs::app_frame/click_at` 与大量直接构造
  `AutoShadeApp` 的 open/landing case。
- 当前 case：`src/bin/gui/tests.rs::dropping_a_photo_opens_the_editing_canvas`
- 状态：`rewritten`

当前 case 不构造或读取任何 App 私有状态：

1. 合成真实 PNG。
2. 通过 `egui_kittest::Harness` 的 `RawInput.dropped_files` 注入用户拖放。
3. 有界 realtime frame 等待真实 decode worker。
4. 从 AccessKit 可见树验收 `No photo open.` 消失且 `Before (source)` 出现。

测试构造修正记录：

- kittest 的 value 查询可能同时命中 Label 与 TextRun，因此使用集合查询，不把节点
  数量当产品契约。
- 初版猜测旧注释文案 `Before · source`，基线红；改为资源表真实公开文案
  `Before (source)`，没有修改产品。

Mutation 证据：临时让 `upd_dropped_and_title` 丢弃全部 dropped files，case 在空状态
仍存在的断言处失败；恢复后通过，`app.rs` 无最终 diff。

该 case 只勾销“公开拖放能打开并显示画布”。它不勾销 main 对失败打开、过期 worker
结果、saved develop 恢复、mask resolve、编辑/保存/导出等后续行为的断言。

### 可见编辑后保存

- Main 相关 cases：
  - `src/bin/gui/tests/normal.rs::the_workspace_save_keys_save_the_develop_and_export`
  - `src/bin/gui/tests/turns_and_layout.rs::the_toolbar_turn_moves_the_recipe_and_the_plate_together`
- 当前 case：`src/bin/gui/tests.rs::rotating_and_saving_persists_the_develop_without_changing_the_photo`
- 状态：`rewritten`（旋转 + Ctrl+S 保存 arm）

公开旅程：

1. 拖放真实 PNG 并等待打开。
2. 通过 AccessKit Button `⭮ Turn right` 点击一次。
3. 注入 Command+S。
4. 通过公开 Store API 定位保存 recipe，断言 `quarter_turns == 1`。
5. 断言源 PNG 字节完全不变。

Mutation 证据：临时让 `rotate_photo` 立即返回，按钮点击不产生编辑，Ctrl+S 不发布
recipe，case 在“develop recipe 必须存在”处失败；恢复后通过。

测试构造边界：曾尝试通过 AccessKit 编辑 `Exposure`，但当前自定义 slider 只暴露
文本 Label，没有关联 numeric control；按 label 查询得到 `numeric_value=None`。该路径
已撤回，没有改读 App 私有字段或按 rect 猜滑块坐标。Exposure 等 slider 的公开
accessibility/交互保护仍是明确 GUI 缺口。

## Config、Library、Session、Denoise 与退化 Fit 已裁决

### DeepSeek key 只在安全边界内共享

- Main cases：
  - `src/config.rs::one_key_fills_the_other_deepseek_role`
  - `src/config.rs::a_role_with_its_own_key_is_never_overwritten`
  - `src/config.rs::one_non_deepseek_endpoint_blocks_the_copy`
  - `src/config.rs::nothing_typed_copies_nothing`
- 当前入口：公开 `config::fill_deepseek_key`
- 状态：四条均为 `rewritten`
- Gate：默认 `tests/domain_behavior.rs`

逐断言映射：

| main 断言 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| 两个 DeepSeek role、目标无 key 时复制刚输入的 key 与 base | `a_deepseek_key_is_shared_with_the_other_deepseek_role_when_it_has_no_key` | 用户必须重复配置同一 provider，或一个角色无凭据 |
| 目标已有 key 时返回 `None` | `a_deepseek_key_is_not_shared_when_the_other_role_already_has_one` | 已保存凭据被另一角色的新输入静默覆盖 |
| 任一 role 不是 DeepSeek 时返回 `None` | `a_deepseek_key_is_not_shared_with_a_non_deepseek_role` | 凭据跨 provider 误传 |
| 输入 key 为空时返回 `None` | `an_empty_deepseek_key_is_not_shared` | 空凭据被保存为另一角色的配置 |

Mutation 证据：临时反转四个公开返回分支，让合法共享返回 `None`，并让已有 key、
跨 provider、空 key 返回 `Some`；四个当前 case 分别在自己的公开结果断言处失败。
恢复后 `src/config.rs` 无 diff。

### Look summary 只来自真实 reference phrases

- Main case：`src/library/tests.rs::the_look_summary_is_the_first_reference_that_has_phrases`
- 当前 cases：
  - `a_look_summary_uses_the_first_reference_with_phrases`
  - `references_without_phrases_have_no_look_summary`
- 状态：`rewritten`
- Gate：默认 `tests/domain_behavior.rs`

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| 跳过无 phrases 的首个 hit，使用首个有效 phrases，并折叠行内空白 | `a_look_summary_uses_the_first_reference_with_phrases` | 保留公开 reviewer intent |
| 所有 reference 都无 phrases 时返回 `None` | `references_without_phrases_have_no_look_summary` | 独立边界 case；禁止伪造用户未提供的 look |

Mutation 证据：让公开 `look_summary` 一律返回 `None` 时，正常路径 case 在
`None != Some("cool quiet")` 处失败；让无 phrases 输入返回合成字符串时，边界 case
在 `Some("synthetic look") != None` 处失败。两次均已恢复。

### Session usage 只累计 assistant，空 transcript 为零

- Main case：`src/session.rs::usage_sums_only_assistant_turns`
- 当前 cases：
  - `session_usage_sums_only_assistant_messages`
  - `a_transcript_without_messages_has_zero_usage`
- 状态：`rewritten`
- Gate：默认 `tests/domain_behavior.rs`

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| `input`、`output`、`cache_read`、`total_tokens`、`cost_usd` 只累计 assistant rows | `session_usage_sums_only_assistant_messages` | 保留公开账单/会话汇总行为 |
| 缺少 `messages` 的 transcript 返回全零 `Usage` | `a_transcript_without_messages_has_zero_usage` | 拆成独立错误边界 |

Mutation 证据：让非空 transcript 直接返回零值时，正常路径在 `input: 0 != 12` 处
失败；让空 transcript 返回 `input=1` 时，边界 case 在完整 `Usage` 等值断言处失败。
两次均已恢复。

### Session 公开 reader 的其余结果

- Main cases：
  - `src/session.rs::tool_loop_counts_calls_and_results`
  - `src/session.rs::verdicts_keep_critic_order_and_score`
  - `src/session.rs::commit_accepts_object_and_legacy_string_forms`
  - `src/session.rs::resolve_session_accepts_an_id_a_filename_or_a_path`
  - `src/session.rs::failure_kind_reads_the_record_and_derives_from_legacy_errors`
  - `src/session.rs::budget_exhaustion_is_a_failure_class_not_an_empty_run`
- 状态：六条均为 `rewritten`
- Gate：默认 `tests/domain_behavior.rs`

逐断言映射：

| main 断言 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| assistant turns、tool calls、tool results 分别为 `2/2/1` | `session_turn_and_tool_counts_ignore_non_assistant_rows` | CLI 会话摘要把非 assistant 内容算入或漏算真实 tool loop |
| 缺少 messages 时三个计数均为 0 | `a_transcript_without_messages_has_no_turns_or_tools` | 空 transcript 伪造活动 |
| verdicts 保持 critic 顺序、decision 与 score | `session_verdict_lines_keep_decision_and_score_together` | 用户看到的评审历史乱序或分值错配 |
| 无 verdicts 与失败但无 verdicts 都返回空 | `a_transcript_without_verdicts_has_no_verdict_lines`；`a_failed_transcript_without_verdicts_does_not_invent_one` | 失败会话伪造评审结论 |
| object commit 读出 develop/xmp/note | `a_commit_object_exposes_its_develop_xmp_and_note_paths` | 当前格式的持久化结果在 show/replay 中丢失 |
| legacy JSON-string commit 读出同三项 | `a_legacy_commit_string_exposes_the_same_three_paths` | 旧 transcript 无法 replay |
| 无 commit 返回三个空字符串 | `a_transcript_without_a_commit_has_no_commit_paths` | 未提交会话伪造文件结果 |
| session id 与显式现存路径解析到真实文件 | `a_session_id_resolves_to_its_json_file`；`session_resolution_prefers_an_existing_direct_path` | `session show/replay` 找不到用户指定记录 |
| 缺失 session 返回含查找目录的错误 | `a_missing_session_reports_where_resolution_was_attempted` | 用户无法判断在哪里查找失败 |
| bare filename 在 sessions 目录解析 | `a_session_filename_resolves_inside_the_sessions_directory` | 补充公开文档已承诺但 main 未单独断言的第三种输入 |
| structured failure 原样保留 | `a_structured_session_failure_keeps_its_failure_class` | 当前 transcript 的失败类型丢失 |
| legacy budget/generic error 分别映射为 `budget_exhausted`/`error` | `a_legacy_budget_error_is_reported_as_budget_exhaustion`；`an_unclassified_legacy_session_error_uses_the_generic_failure_class` | 旧记录被错误归因 |
| 无 error 时 failure 为 `None` | `a_session_without_an_error_has_no_failure_class` | 成功会话被标成失败 |
| 显式 true 与 legacy budget error 都是 budget exhausted | `an_explicit_budget_exhaustion_flag_is_reported`；`a_legacy_budget_error_is_reported_as_budget_exhaustion` | 新旧格式漏报预算耗尽 |
| 显式 false 与 model error 都不是 budget exhausted | `a_false_budget_exhaustion_flag_is_not_reported`；`a_model_error_is_not_reported_as_budget_exhaustion` | 普通失败被错误归为预算耗尽 |

Mutation 证据：

- 让五个公开 reader 对有效输入统一返回空/零/错误时，正常计数、verdict、两种
  commit、三种解析与三类 failure 共 10 个 case 变红；缺失输入 case 仍绿。
- 恢复正常输入后，让缺失输入统一伪造活动/verdict/commit/session/failure，6 个负向
  case 变红；正常输入 case 仍绿。
- 单独忽略结构化 budget flag 时，显式 true case 变红；让 budget reader 一律返回
  true 时，false flag 与 model error 两个 case 分别变红。

所有 mutation 均已恢复，`src/session.rs` 无 diff。

### Model picker 分类、资源边界与 capabilities

- Main cases：
  - `src/openai_models.rs::classifies_real_account_ids`
  - `src/openai_models.rs::model_catalogues_bound_items_and_identifiers_before_sorting`
  - `src/openai_models.rs::the_listing_says_which_tiers_and_images_a_model_takes_when_it_says_anything`
- 当前入口：公开 `is_image_model`、`is_chat_model`、`list_models`
- 状态：三条均为 `rewritten`
- Gate：默认 `tests/openai_models_behavior.rs`，并复用
  `tests/domain_behavior.rs` 的 Qwen/DeepSeek 样本

逐断言映射：

| main 断言组 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| `gpt-image-*`/`chatgpt-image-*`/case-folded Qwen 是 image 且不进 chat | `known_image_generation_ids_are_recognized_and_not_offered_as_chat`；`image_generation_models_are_not_offered_as_chat_models` | 设置页把图像模型送进 chat endpoint，或不再提供真实图像模型 |
| `gpt-*`、`o3/o4` 与五个第三方 vendor ID 进入 chat | `known_openai_chat_ids_are_offered_as_chat`；`third_party_chat_ids_are_offered_as_chat`；`vendor_prefixed_chat_models_are_offered_by_the_chat_filter` | 可配置 endpoint 的真实模型从 picker 消失 |
| audio/embed/rerank/guard/DALL-E/Sora 等非 chat modality 均被排除，且大小写折叠 | `non_chat_modalities_are_not_offered_as_chat` | 选择后必然被 chat endpoint 拒绝的模型进入 picker |
| `babbage-002`/`davinci-002` 与空 ID 被排除 | `completion_only_models_are_not_offered_as_chat`；`an_empty_model_id_is_not_offered_as_chat` | completion-only 或空模型被作为 chat 提交 |
| 普通 chat ID 不被误判为 image | `a_non_image_chat_id_is_not_classified_as_an_image_model` | chat 模型跑到生成图角色 |
| 超过 4096 items 的 listing 被拒绝 | `a_model_listing_over_the_item_limit_is_refused` | 不受信响应制造无界 picker/内存开销 |
| 超长 ID 与含控制字符 ID 被拒绝 | `an_oversized_model_id_is_refused`；`a_model_id_with_control_characters_is_refused` | 不受信 ID 污染 UI 或诊断 |
| plain listing 保持 capabilities `None` | `a_plain_model_listing_keeps_capabilities_unknown` | “未知”被误当成“不支持”，固定建议消失 |
| Venice reasoning/vision 声明过滤坏 tier 并保留真假 | `a_venice_reasoner_listing_exposes_sanitized_tiers_and_vision`；`a_venice_non_reasoner_listing_exposes_no_tiers_and_no_vision` | 设置页发送非法 effort 或错误展示 vision |
| OpenRouter modalities/parameters 映射到三档 reasoning 与 vision | `an_openrouter_vision_listing_exposes_reasoning_tiers_and_vision`；`an_openrouter_text_listing_exposes_no_tiers_and_no_vision` | provider capability 在 UI 中被误读 |

资源与 capability cases 各自启动随机 localhost 端口的一次性 HTTP server，stub 只替代
外部服务；API key 校验、HTTP 请求、有界响应读取和 `list_models` 公开返回均走真实
产品路径。

Mutation 证据：

- 让 image/chat 分类器统一返回 false，3 个正向 classifier case 变红；统一返回 true，
  5 个负向 classifier case 变红。
- 让合法 `/models` 响应统一解析为空列表，三个资源拒绝与五个 capability case 共
  8 个红灯；分类器 cases 不受影响。

所有 mutation 均已恢复，`src/openai_models.rs` 无 diff。

### SHA-256 持久键匹配 FIPS 向量

- Main case：`src/sha256.rs::sha256_matches_the_fips_180_4_vectors`
- 当前 cases：
  - `the_sha256_known_answer_matches_the_standard_vector`
  - `the_sha256_empty_input_matches_the_standard_vector`
  - `the_sha256_multi_block_input_matches_the_standard_vector`
  - `the_sha256_million_byte_input_matches_the_standard_vector`
- 状态：`rewritten`
- Gate：默认 `tests/product_behavior.rs`

四个 case 通过公开 `sha256_hex` 分别保护 `abc`、空输入、跨 block padding 与百万字节
长度字段；故障会重新命名持久缓存/sidecar drift key，而不是只改变内部实现。

Mutation 证据：让所有输入固定返回 `abc` 摘要时，空、多块、百万字节三个 case
同时变红而 `abc` 保持绿色；改为固定返回空输入摘要时，`abc` case 变红。实现已恢复，
`src/sha256.rs` 无 diff。

### Recipe library 公开文件生命周期与 portability

- Main cases：`src/recipe_library.rs` 中全部 8 条 case
- 当前入口：公开 `create_in`、`list_in`、`read_in`、`replace_in`、`rename_in`、
  `delete_in`、`check_name`、`portable`、`read_file`
- 状态：8 条均为 `rewritten`
- Gate：默认 `tests/recipe_library_behavior.rs`，并复用
  `tests/product_behavior.rs` / `tests/domain_behavior.rs` 的现有公开行为

逐断言映射：

| main 断言组 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| create 写出普通 recipe JSON，list/read 返回同名同 look | `creating_a_recipe_library_entry_writes_a_plain_recipe_file`；`listing_and_reading_a_recipe_library_entry_returns_its_name_and_look` | 保存后 CLI 不能读取，或 GUI 列表/打开丢失内容 |
| replace 只改变同名 look | `replacing_a_recipe_library_entry_changes_its_saved_look` | 替换未落盘或名字漂移 |
| rename 移走旧名、保留 recipe；delete 只删指定项 | `renaming_a_recipe_library_entry_moves_its_file_and_preserves_the_recipe`；`deleting_a_recipe_library_entry_removes_only_the_named_entry` | 改名/删除误伤或内容丢失 |
| create/rename 遇到 taken name 均拒绝且双方内容保留 | `creating_a_recipe_library_entry_refuses_to_replace_an_existing_name`；`renaming_to_a_taken_recipe_name_preserves_both_entries` | 用户不可恢复的 recipe 被静默覆盖 |
| malformed entry 仍作为 error row，且 valid row 不消失 | `listing_a_malformed_recipe_library_entry_reports_its_error`；`a_malformed_recipe_entry_does_not_hide_a_valid_entry` | 损坏文件被静默隐藏，用户误以为从未保存 |
| missing library 列表为空；missing name 不存在 | `a_missing_recipe_library_lists_as_empty`；`a_missing_recipe_name_is_reported_as_absent` | 首次使用被当成错误或伪报已占名 |
| 名称 trim；path/reserved ending/empty/oversized 分别拒绝 | `surrounding_whitespace_is_removed_from_a_recipe_name`；`a_recipe_library_name_with_a_path_separator_is_refused`；`filesystem_reserved_recipe_name_endings_are_refused`；`an_empty_recipe_name_is_refused`；`an_oversized_recipe_name_is_refused` | 名称逃逸目录、跨平台撞名或无法持久化 |
| geometry 与 bitmap mask 被丢弃并计数，global look 与 parametric mask 保留 | `portable_recipe_drops_photo_bound_masks_and_geometry_but_keeps_the_look`；`portable_recipe_keeps_a_parametric_mask_and_the_global_look` | recipe 把源照片坐标带到另一张，或丢掉可迁移 look |
| colour field / pixel layers / lens blur 被丢弃并分别披露 | `portable_recipe_drops_advanced_photo_specific_state_and_counts_each_loss` | 每照片像素/深度状态静默污染另一张照片 |
| plain recipe 无损；intent-only zone 保留，带 raster zone 丢弃并计数 | `a_plain_portable_recipe_loses_nothing`；`an_intent_only_zone_mask_travels_to_another_photo`；`a_zone_mask_with_a_photo_raster_does_not_travel` | 可重建意图被误删，或源照片 raster 被错误复用 |
| `.xmp` 经公开 `read_file` 导入 Exposure | `importing_an_xmp_recipe_uses_the_public_xmp_reader` | 用户选择的 Lightroom sidecar 被当成中性/普通 JSON |

`a_missing_recipe_name_is_reported_as_absent` 在独立 integration binary 中用
`ScopedEnv` 隔离 `AUTOSHADE_DATA_DIR`；其余文件 case 均使用每 case 独立临时目录，可
并行运行。

Mutation 证据：

- 保持 create 正常、让 list/read/rename/delete 返回空或 no-op 时，domain 2 个、新
  suite 3 个、product 3 个相关 case 变红；missing/name/portable/XMP cases 仍绿。
- 让 durable writer no-op 时，“新建即普通 recipe 文件”与 taken-name create 两个
  case 分别变红。
- missing library 伪造一行、`exists` 永真、名称校验无条件接受时，missing/name 的
  7 个负向 case 变红，正常 CRUD 仍绿。
- portability 原样返回时，应丢弃的 geometry/advanced state/raster zone 三类 case
  变红；统一返回默认 recipe 时，应保留的 global look/parametric mask/plain
  recipe/intent zone cases 变红。
- XMP arm 返回中性 recipe 时，公开 import case 的 Exposure 断言变红。

所有 mutation 均已恢复，`src/recipe_library.rs` 无 diff。

### Adobe resource discovery

- Main cases：
  - `src/adobe/tests.rs::the_override_comes_first_and_only_real_directories_survive`
  - `src/adobe/tests.rs::the_walk_is_recursive_case_insensitive_and_bounded`
- 当前入口：公开 `adobe::camera_raw_roots`、`adobe::walk_extension`
- 状态：两条均为 `rewritten`
- Gate：默认 `tests/adobe_behavior.rs`

逐断言映射：

| main 断言 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| 现存 override 是第一 root，所有返回项都是真实目录 | `an_existing_adobe_override_is_the_first_resource_root` | 用户指定 profile 不优先，或 phantom root 延迟到更难懂的读取错误 |
| missing override 不进入 roots | `a_missing_adobe_override_is_not_a_resource_root` | 不存在路径伪装成 Camera Raw install |
| empty override 不变成 cwd | `an_empty_adobe_override_does_not_make_the_working_directory_a_resource_root` | 仓库/工作目录被误扫为 Adobe profiles |
| walk 递归且 extension 大小写不敏感 | `an_adobe_resource_walk_is_recursive_and_extension_case_insensitive` | vendor 子目录或 Adobe 的大写扩展资源消失 |
| extension 是调用参数而非硬编码 | `an_adobe_resource_walk_uses_the_requested_extension` | LCP/DCP 两个 caller 只能有一个工作 |
| budget 0 不读取 entry | `a_zero_adobe_walk_budget_reads_no_entries` | symlink/大目录预算失效 |
| missing root 安静返回空 | `a_missing_adobe_resource_root_is_a_quiet_empty_result` | 未安装 Camera Raw 导致 panic/伪结果 |

全部 filesystem fixture 使用独立 `TempTree`；三个 override cases 通过 `ScopedEnv` 串行
修改并恢复唯一测试变量。

Mutation 证据：

- roots 统一返回空时，现存 override case 变红；去掉 empty guard 和 `is_dir` filter
  时，现存/missing/empty 三个 guard case 变红。
- walk 同时停止递归、写死大小写敏感 `lcp`、忽略 budget 时，recursive/case、
  extension 参数、zero-budget 三个 case 各自变红。
- missing `read_dir` 伪造一个 path 时，missing-root case 变红。

所有 mutation 均已恢复，`src/adobe.rs` 无 diff。

### Recoverable panic scope

- Main case：`src/panic_guard.rs::the_scope_nests_and_a_caught_panic_still_restores_it`
- 当前 cases：
  - `a_recoverable_scope_marks_only_its_own_lifetime`
  - `a_nested_recoverable_scope_does_not_clear_its_outer_scope`
  - `a_caught_panic_restores_the_thread_to_fatal_ground`
- 状态：`rewritten`
- Gate：默认 `tests/panic_guard_behavior.rs`

逐断言映射：

| main 断言组 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| bare false、entered true、scope exit false | `a_recoverable_scope_marks_only_its_own_lifetime` | 可恢复 parser/worker panic 被误报 fatal，或普通 panic 被吞成可恢复 |
| inner enter/exit 不清除 outer | `a_nested_recoverable_scope_does_not_clear_its_outer_scope` | worker 内 parser guard 结束后，外层剩余工作被误报 fatal |
| intentional panic 被 catch，且 unwind 后恢复 false | `a_caught_panic_restores_the_thread_to_fatal_ground` | 一次可恢复 panic 污染线程后续 panic 分类 |

测试构造修正：最初把 `panic_is_recoverable()` 的 premise 写在 `catch_unwind` 内；
premise 自己失败也会被 catch，可能造成假绿。最终 case 先把观察写入外部 `Cell`，再在
catch 之后独立断言 premise、panic 被捕获和 Drop 恢复三个结果。

Mutation 证据：状态查询永假时三个 case 均红；Drop 把 depth 清零时 nested case 红；
Drop no-op 时普通 scope exit 与 panic unwind 两个 case 红。实现已恢复，
`src/panic_guard.rs` 无 diff。

### XMP composed mask 的公开 coverage

- Main case：`src/render.rs::xmp_component_round_trip_preserves_unquantized_composed_weights`
- 当前 case：`tests/xmp_behavior.rs::composed_mask_coverage_is_unchanged_by_an_xmp_round_trip`
- 状态：`rewritten`
- Gate：默认 `tests/xmp_behavior.rs`

main 直接调用私有 `combined_mask_weight` 比较未量化浮点值，锁定 render helper。当前
case 改走公开 `recipe_to_xmp` / `xmp_to_recipe` 与 `mask_coverage`，在 127×73 合成
frame 上逐像素比较 Add/Subtract/Intersect × component inversion 的完整 8-bit
coverage。它保留“用户存取 XMP 后蒙版画面不变”的产品契约，不要求未来 renderer
继续使用同一个私有 helper 或浮点中间表示。

Mutation 证据：临时把 XMP 读回的第一个 component mode 强制为 Add，Add 对照仍通过，
Subtract 在整张 coverage 比较处变红。reader 已恢复，`src/xmp/read.rs` 无 diff。

### 零强度 developed denoise 是无损 no-op

- Main case：`src/denoise.rs::zero_strength_denoise_is_the_identity`
- 当前 case：`tests/denoise_behavior.rs::a_zero_strength_developed_frame_is_the_identity_without_a_sidecar`
- 状态：`rewritten`
- Gate：默认 `tests/denoise_behavior.rs`

当前 case 通过公开 `denoise_buffer` 输入超出 `[0,1]` 的像素，并给出不存在的
sidecar。成功返回且像素逐值相同同时证明：零强度不 clamp、不量化，也不启动外部
进程。

Mutation 证据：把零强度快捷返回从 `<= 0.0` 改成 `< 0.0` 后，case 因尝试进入
sidecar 路径而返回“denoise sidecar not found”，未到达像素等值断言；恢复后通过。

### 退化 pair 保持中性并披露拒绝

- Main case：`src/fit/tests.rs::a_degenerate_pair_refuses_to_fit_and_says_why`
- 当前 case：`tests/fit_behavior.rs::a_degenerate_pair_refuses_to_fit_and_discloses_the_refusal`
- 状态：`rewritten`
- Gate：`cargo test --test fit_behavior -- --ignored`

逐断言映射：

| main 断言 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| `exposure_ev == 0` | 同名当前 case 的公开 `FitReport.recipe` | 无证据的黑帧得到可见曝光编辑 |
| `tone_curve` 为空 | 同一公开 recipe | 退化 CDF 产生常量 tone map |
| notes 含 typed `FIT_DEGENERATE` | 同一公开 `FitReport.notes` | 用户拿到中性结果却不知道为何拒绝拟合 |

Mutation 证据：在退化公开返回分支分别注入 `exposure_ev=1`、单点 tone curve、清空
typed notes；当前 case 依次在三条对应断言处失败。三次均已恢复，`src/fit/solve.rs`
无 diff。

## 当前公开引擎基线

以下当前测试文件已按新原则重写 main 中的一批底层事实。它们进入本审计的基线，但
本表不把整份 main 文件自动标为 `equivalent`；具体映射仍以本文件已裁决条目和
`docs/TEST_GAP_CHECKLIST.md` 的 premise/act/assert/mutation 记录为准。

| 当前文件 | cases | 主要公开行为 | 对应 main 家族 | 尚未因此勾销 |
| --- | ---: | --- | --- | --- |
| `tests/product_behavior.rs` | 81 | recipe clamp、XMP 基础 round-trip、输出拒绝/发布、公开 render/decode/segment/pipeline 文件结果 | Recipe、Pipeline、Render、XMP、Decode、Segment、Jobs | main 的私有 solver/parser helper、完整 raster payload、平台路径矩阵 |
| `tests/domain_behavior.rs` | 99 | session/describe/embed、face、retouch、stack、色彩、几何、外部 XMP 形状 | Render、XMP、Face、Stack、Advisor、Embed | main 的内部 LUT/gate 数值与未映射复杂 mask composition |
| `tests/openai_models_behavior.rs` | 15 | model picker 分类、响应资源边界、Venice/OpenRouter capability | OpenAI models、Settings | 真实远端可用性与 provider 账单/权限 |
| `tests/recipe_library_behavior.rs` | 16 | recipe CRUD、命名边界、portable losses、XMP import | Recipe library、XMP | GUI 菜单可见性与跨进程权限错误 |
| `tests/adobe_behavior.rs` | 7 | Adobe roots、递归资源 walk、budget | LCP、DCP | 真实安装目录权限与损坏资源内容 |
| `tests/panic_guard_behavior.rs` | 3 | recoverable scope、nesting、panic unwind | Decode、GUI workers | OS/GUI panic reporter 的进程级呈现 |
| `tests/xmp_behavior.rs` | 4 | payload/namespace round-trip、composed mask coverage | XMP、Render masks | Adobe 外部编辑兼容矩阵 |
| `tests/properties.rs` | 9 | clamp 幂等、XMP scalar round-trip、lens/CA/warp 往返、worker budget | Recipe、Render、Jobs | 等价变换以外的实际成片结果 |
| `tests/decode_behavior.rs` | 4 | 合成 RAW 转向、as-shot WB、RAW plane 资源拒绝 | Decode、Render/as-shot | 真实相机格式语料与平台 codec gate |
| `tests/denoise_behavior.rs` | 6 | identity、适用性、sidecar 成功/缺失/超时、进程组清理 | Denoise、Process | 真实模型质量与 GPU/平台行为 |
| `tests/process_behavior.rs` | 1 | 杀 sidecar 进程组同时回收孙进程 | Lib/process、Denoise | Windows Job Object 分支 |

这些 case 数只说明本次提交 inventory，不是质量得分。每个已有 mutation 的具体故障
与红灯位置登记在 `docs/BEHAVIOR_PROTECTION.md`；没有 mutation 证据的 main 断言仍
不能靠本表宣称已保护。

## 当前外部边界基线

| 当前文件 | cases | 公开边界 | main 信息来源 |
| --- | ---: | --- | --- |
| `tests/cli_behavior.rs` | 3 | 真实 `autoshade decode/apply` 子进程、文件交付、源图不可覆盖 | `src/main.rs`、Pipeline/Render |
| `tests/mcp_behavior.rs` | 2 | 真实 stdio initialize/tools/list/frame.read 与根目录逃逸拒绝 | `src/mcp.rs` |
| `tests/library_behavior.rs` | 2 | reference build 增量复用、sidecar settings 只有在实际成片相近时附加 | `src/library/**` |
| `tests/judge_behavior.rs` | 2 | fake completion 边界下成功解析、typed failure、密钥不泄漏 | Advisor/Judge/Harness |
| `tests/generative_behavior.rs` | 2 | fake Comfy CLI 协议下完整 PNG 发布、失败不留 partial | Generative/Comfy |

这些 fixture 只替代外部网络、模型或付费系统；被测命令、进程协议、解析与文件发布
均走真实产品入口。逐 case mutation 证据见 `docs/BEHAVIOR_PROTECTION.md`。它们不
替代真实付费/模型质量 gate，也不把 Comfy 并发 collision 或 Harness 部分提交缺陷
解释成已解决。

### HTTP / Serve

- 当前入口：`tests/serve_behavior.rs` 与 `tests/http_*/mod.rs`
- 默认 gate：53 passed，7 ignored。
- 显式资源 gate：两个真实 256 MiB request case 与“999 次失败后 heal 容量恢复”
  case，`scripts/http_resource_battery.sh` 实测 2 + 1 passed。

默认套件覆盖真实 `autoshade serve` 子进程、session/image capability、revision
并发、save/reset/import、Analyze、settings、library、Fill、Heal、Stack、Export 和
Download 的公开 HTTP 结果。下列 ignore 不得混成“核心行为已绿”：

- `print_develop_directory_fixture`、`record_generated_pixel_source_fixture`：父测试
  启动的 fixture worker，不是独立产品 case。
- Harness 部分提交后失败：已知生产缺陷，保留确定性红灯与启用命令。
- 同时 Comfy Fill：已知临时路径 collision，保留红灯与启用命令。
- 三条资源 case：由显式 battery 执行，本轮已绿。

HTTP case 的具体 mutation 与公开失败结果登记在 `docs/HTTP_BEHAVIOR_AUDIT.md` 和
`docs/BEHAVIOR_PROTECTION.md`；该套件通过子进程执行，因此 llvm-cov 未合并
`serve.rs` profile 不能反向证明这些行为没执行。

## 明确不迁移的旧机制

以下机制发现即逐 case 归入 `lint` 或 `drop`，不能复制到当前测试：

- `gui_source()`、`gui_function()`、`include_str!` 读取 Rust 源码。
- `src/bin/gui/tests/architecture.rs` 中按字符串查导入、函数或调用点。
- 直接断言 `AutoShadeApp` 私有字段、私有 helper 或调用顺序。
- `#[cfg(test)] TestSeams` 暴露 rect、enabled 状态或替换被测对象内部执行。
- `cfg(test)` 强制展开面板或改变真实运行行为。

其中架构意图若有价值，必须由现有或新增的编译器、clippy、类型边界或
`scripts/check_architecture.py` 规则接管；不能因为旧测试读取源码就默认删除其剩余
产品行为断言。

## 待处理顺序

顺序按产品故障后果和公共入口组织，不按覆盖率或旧 case 数量：

1. Fit / Fit Zoned 的公开交付 recipe、实际渲染、拒绝披露与 sidecar 往返。
2. Render 的最终像素、几何、蒙版、色彩和资源上限。
3. Store / Pipeline / XMP 的原子文件结果、恢复、兼容和部分失败。
4. Decode / Denoise / Stack / Segment / Generative 的外部边界与失败清理。
5. Serve / CLI / MCP 的进程级公开协议。
6. GUI 的可见状态与真实工作流，使用 `egui_kittest`，不再直接构造私有状态。
