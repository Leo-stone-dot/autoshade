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
- 当前工作树：703 cases，约 2,094 个断言宏，39 个显式 ignore。
- 名称完全相同的 case 只有 1 个；不能按名称自动宣称其余 case 已覆盖。

完整逐 case 数据位于 `docs/main_test_case_audit.tsv`。默认运行
`python3 scripts/check_main_test_audit.py` 会拒绝漏项、重复项、无证据裁决和任何
`open`；目标推进期间可用 `--allow-open` 查看真实剩余量，但该模式不能作为完成门。
当前校验结果：2,281 行与 main snapshot 精确一致，268 条 `rewritten`、13 条
`lint`、2 条 `drop`、3 条 `external-gate`、1,995 条 `open`。

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

### Zone role name fallback and intent precedence

- Main case：`src/xmp/payload_tests.rs::a_zone_role_comes_back_from_the_name_when_no_intent_survives`
- 当前 cases：
  `tests/xmp_behavior.rs::zone_roles_fall_back_from_measured_names_only_without_intent` 与
  `explicit_role_intent_overrides_zone_name_fallback`
- 状态：`rewritten`
- Gate：默认

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| payload 不存在 | no-intent case 直接构造不含 payload/intent 的 Lightroom-shaped XMP | fixture premise；不调用私有 `payload::find` |
| `sky · band 2/3` 为 `ZoneSky` | no-intent case 的公开 `xmp_to_recipe` 结果 | 保留 |
| `grad` 为 `Custom` 且名称仍在 | no-intent case 的 role/name 断言 | 保留 |
| `Sky 1` 为 `Custom` 且名称仍在 | no-intent case 的 role/name 断言 | 保留；Lightroom 默认名不是 AutoShade zone |
| 两个 `sky` 都变为空名 `ZoneSky` | no-intent case 的单个 `sky` 同时断言空名和 `ZoneSky` | 保留转换；删除同一规则在 fixture 中重复两次的 cardinality |
| intent 存在时四个 Custom 仍为 Custom | intent case 用会被名称误判的 `sky + Custom` | 保留冲突方向；删除无冲突成员的聚合计数 |
| intent 存在时两个 ZoneSky 仍为 ZoneSky | intent case 用名称本会判 Custom 的 `Sky 1 + ZoneSky` | 保留反向冲突；删除重复数量 |

Mutation 证据：

- 禁用裸 `sky` 的名称回退时，no-intent case 得到 `Custom` 而非 `ZoneSky`。
- 把所有未知名称清空时，no-intent case 在 `grad` 的公开 name 断言处失败。
- reader 忽略 `ash:Role`、只采用名称推断时，intent case 首先把明确的 `Custom`
  错读为 `ZoneSky`；反向 fixture 同时保护明确 `ZoneSky` 不被名称降级。

三次 mutation 均已恢复，`src/xmp/payload.rs` 与
`src/xmp/read_corrections.rs` 无 diff。

### Payload inversion reconciliation

- Main case：
  `src/xmp/payload_tests.rs::an_inversion_keeps_its_authored_home_through_a_rewrite_and_follows_a_lightroom_flip`
- 当前 cases：
  `tests/xmp_behavior.rs::an_untouched_external_rewrite_preserves_the_authored_inversion_home`
  与 `an_external_inversion_edit_overrides_the_payload_in_the_native_home`
- 状态：`rewritten`
- Gate：默认

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| untouched rewrite 仍有两个 masks | untouched case 分别用单 AI、单 Radial，均精确断言一个结果 | 保留“不丢、不复制”；删除 fixture 聚合数量 |
| 两种 geometry 都保留 authored `(whole=true, own=false)` | untouched case 遍历公开 AI/Radial recipe pair | 保留 |
| untouched mask 整体等于 payload | inversion/exposure 由本组执行；role/name 由 role fallback/intent cases；其余 payload/geometry 由 complete-develop 与 native composition cases | 拆分，避免一个无边界 struct 断言锁住无关字段 |
| Lightroom 把 net 从 true 改 false 后两种 mask 均不再 inverted | external-edit case 的 `net_inverted()` | 保留 |
| 上述 flip 不改变各自 exposure | external-edit case 的公开局部曝光断言 | 保留 |
| 从 false 改 true 后两种 mask 均 net-inverted | external-edit case 的两个 expected native pairs | 保留 |
| AI 的外部 true 落在 `(whole=false, own=true)` | external-edit case 的 AI arm | 保留公开可编辑状态 |
| Radial 的外部 true 落在 `(whole=true, own=false)` | external-edit case 的 Radial arm | 保留公开可编辑状态 |

Mutation 证据：

- reconciliation 对未改写文档强制采用 native pair 时，AI 从 `(true,false)` 漂成
  `(false,true)`，untouched case 红。
- reconciliation 对所有文档强制采用 payload pair 时，外部移除 inversion 后仍
  net-inverted，external-edit case 红。
- reconciliation 复制输出 mask 列表时，untouched case 在公开数量 `2 != 1` 处红。
- inversion reconciliation 顺带把局部曝光清零时，external-edit case 在
  `0.0 != -0.5` 处红。

四次 mutation 均已恢复，`src/xmp/payload.rs` 无 diff。

### Additional raster paths in XMP payloads

- Main case：
  `src/xmp/payload_tests.rs::the_payload_carries_depth_protect_and_layer_paths_bare_and_refuses_a_share`
- 当前执行者：
  `tests/xmp_behavior.rs::xmp_payload_carries_additional_rasters_by_filename_only`、
  `tests/store_behavior.rs::unsafe_raster_references_are_disabled_across_every_recipe_raster_kind`
  与
  `tests/xmp_behavior.rs::a_photo_less_xmp_read_disables_every_unanchored_raster_reference`
- 状态：`rewritten`
- Gate：默认

逐断言映射：

| main 断言 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| depth/protect/layer 在 payload 中都只有 bare filename | 新 XMP case 解码公开 sidecar 的 `asr:Recipe` artifact，比较三条公开 recipe path | sidecar 泄露本机目录且无法在另一台机器重锚 |
| network depth path 恢复为 inert sentinel | unsafe store case + photo-less XMP case | 导入共享路径后访问任意网络文件 |
| device protect path 恢复为 inert sentinel | 同上 | 导入 sidecar 后打开设备路径 |
| escaping pixel-layer path 恢复为 inert sentinel | 同上 | `..` 引用逃出 develop 目录 |

Mutation 证据：

- payload portability 退回只遍历 mask raster 时，新 XMP case 解出的三个路径全部为
  临时目录绝对路径，filename tuple 断言红。
- 既有 store mutation 中，`raster_paths_mut` 退回只遍历 mask 时 additional raster
  refusal case 红；containment 无条件 join 时 `../../outside.png` layer 不再落
  sentinel；这些证据同时执行恢复侧三条拒绝断言。

mutation 已恢复，`src/xmp/payload.rs` 无 diff。

### Merged Lightroom base and Written record

- Main cases：
  `our_own_sidecar_over_a_lightroom_base_reads_back_as_the_develop_it_carries`、
  `a_lightroom_edit_to_a_key_the_base_carried_still_wins` 与
  `a_written_record_that_does_not_verify_is_disclosed_and_dropped`
- 当前 cases：
  `tests/xmp_behavior.rs::a_merged_lightroom_base_reopens_as_the_develop_autoshade_wrote`、
  `external_profile_and_lens_switch_edits_override_the_written_payload` 与
  `a_corrupt_written_record_is_disclosed_while_the_payload_recipe_still_opens`
- 状态：3 条 `rewritten`
- Gate：默认

逐断言映射：

| main 断言 | 当前执行者 | 裁决 |
| --- | --- | --- |
| merge 保留 CameraProfile、Look、LensProfileEnable | merged-base case 直接检查公开 XMP artifact | 保留 |
| XMP 精确含 `asr:Written` | merged-base 的重开结果和 `written_lens_profile` | 改写为该记录必须实现的公开行为，不锁 attribute 布局 |
| payload notes 为空 | merged-base 的公开 `Collector` 为空 | 保留“合法 sidecar 不误报” |
| 私有 `Payload.written` 存在且不递归含 payload | 无 | 删除内部表示；成功重开与文件大小上限由公开行为验收 |
| base profile/Look 不成为 edit，完整 develop 相等 | merged-base 的公开 `EditRecipe` 相等 | 保留 |
| `written_lens_profile` 返回 develop 的 profile | merged-base 的公开查询 | 保留 |
| write-read-render 像素相等 | 无独立 case | 删除冗余：一次针对同一故障的合成 preview mutation 仍绿；recipe 已精确相等时同一确定 renderer 的自比较不增加保护 |
| Lightroom 改 CameraProfile 后新值胜出 | external-edit case | 保留 |
| Lightroom 改 lens switch 后 payload profile 不再适用 | external-edit case 的 `None` | 保留 |
| checksum 错误令私有 `written` 为 None | corrupt-record case 的公开 warning + recipe 仍打开 | 改写；不断言私有状态 |
| checksum 错误进入 notes | corrupt-record case 的公开 diagnostic 文案 | 保留 |

Mutation 证据：

- `written_document` 忽略真实 merge 记录、退回 bare projection 时，base 的 Adobe
  profile/Look 被误导入，merged-base case 的完整 recipe 断言红。
- 同一 mutation 下曾尝试的像素 parity case仍绿，因此该新 case被删除，没有拿
  vacuous 绿灯计作保护。
- reconciliation 对 `camera_profile` 永远采用 payload 时，Adobe Portrait 外部编辑
  被读回空 profile，external-edit case 红。
- `written_lens_profile` 无条件返回 payload profile 时，外部 switch edit 得到
  `Some(...)` 而非 `None`，external-edit case 红。
- reader 接受任意已声明 Written CRC 时，corrupt-record case 缺少 checksum
  disclosure 并红。
- reader 把每个合法 Written 都判为 checksum error 时，merged-base case 的
  `Collector::is_empty` 断言红。

全部 mutation 均已恢复，`src/xmp/payload.rs` 无 diff。

### Real Lightroom rewrite fixtures

- Main case：`src/xmp/payload_tests.rs::the_real_lightroom_rewrites_read_as_measured`
- 当前默认 case：
  `tests/xmp_behavior.rs::an_unsupported_payload_format_is_disclosed_while_the_crs_projection_opens`
- 当前外部 gates：
  `real_lightroom_probe_preserves_payload_and_raster_bytes`、
  `real_lightroom_v130_rewrite_recovers_zone_roles_from_band_names` 与
  `real_lightroom_v131_rewrite_restores_the_payload_develop`
- 状态：`external-gate`
- 启用：
  `AUTOSHADE_LR_PAYLOAD_FIXTURES=/path/to/lr-payload-2026-09 cargo test --release --test xmp_behavior real_lightroom_ -- --ignored`

三个 gate 都用 `#[ignore]` 明确隔离外部私有 fixture。启用后环境变量缺失或任一文件
缺失会失败，不再像 main case 一样 `return` 成静默绿灯。当前机器没有该环境变量，
Spotlight 也找不到三组 fixture，因此本轮只验证它们可编译，没有宣称外部 gate 已跑。

逐断言映射：

| main 断言族 | 当前执行者 | 裁决 |
| --- | --- | --- |
| probe 原件确被 Lightroom 改写，namespace/payload/SHA/5 个 raster bytes 均保留 | real probe gate 比较两份公开 XMP artifact 的 Recipe、RecipeSha256 与 Name/Sha256/Data 序列 | 外部事实，保留在显式 gate |
| probe payload 不是当前格式且必须拒绝 | 默认 unsupported-format case 的公开 diagnostic + CRS exposure | 默认保护，不再调用私有 `payload::find` |
| v1.3.0 原件有 intent、rewrite 无 intent/payload | v130 gate 的 artifact premise | 外部事实 |
| v1.3.0 两个 band 的 name/ZoneSky 恢复 | v130 gate；默认 name fallback case 同时保护合成路径 | 外部兼容 + 默认行为 |
| Lightroom materialise `ColorNoiseReduction=25`，原件仍为 0 | v130 gate 的公开 recipe 字段 | 外部事实 |
| v1.3.1 Recipe/raster bytes 经 rewrite 保留 | v131 gate 比较公开 artifact 属性 | 外部事实 |
| v1.3.1 rewrite 无假 warning，colour field、6 masks、name/role/enabled/inverted、zone count、exposure/WB 与 color NR 恢复 | v131 gate 的公开 reader/collector | 外部兼容 |
| 私有 payload decode、notes、rasters 结构 | 上述公开 artifact 与 reader 结果 | 改写，不断言私有 `Payload` |
| 完整 JSON leaf diff 必须恰为 15 个内部路径 | 无 | 删除精确内部布局锁；有产品语义的 mask identity、global fields 与外部 edit precedence 已逐项执行 |

Mutation 证据：decoder 错误接受任意非空 future format 时，默认 case 的 CRS exposure
仍正确，但 format disclosure 消失，case 在公开错误断言处红；mutation 已恢复。

### External rewrite reconciliation

- Main cases：
  `a_lightroom_rewrite_keeps_what_it_did_not_touch_and_yields_what_it_did`、
  `an_edited_recolour_carrier_is_lightrooms_and_the_gains_go` 与
  `an_explicit_companion_zero_survives_a_rewrite_and_yields_to_an_edit`
- 当前 cases：
  `an_external_rewrite_keeps_untouched_payload_state_and_applies_its_edits`、
  `an_external_recolour_edit_replaces_the_payload_gains` 与
  `an_explicit_companion_zero_survives_until_an_external_edit_replaces_it`
- 状态：3 条 `rewritten`
- Gate：默认

测试用公开 writer 分别生成 original payload 与 edited CRS artifact，再只替换公开
`asr:Recipe`/CRC 属性，模拟 Lightroom 保留 payload、改写 CRS 且剥离 mask intent。
它不调用 main 的 `lightroom_like`、`payload::find` 或 `loaded`。

逐断言映射：

| main 断言族 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| 外部 exposure/amount、删除 mask、新增 mask 与顺序胜出 | external-rewrite case | 用户在 Lightroom 的编辑被旧 payload 覆盖，或 mask 消失/复活 |
| edited mask 的 untouched contrast/geometry保持 | external-rewrite case | 改一个 slider 顺带重置同 mask 其他设置 |
| untouched zone、role、Intersect、colour field、WB anchor、rationale/confidence 保持 | external-rewrite case；完整 payload/role/composition cases共同执行 | Lightroom 重序列化造成 AutoShade-only intent 丢失 |
| Lightroom materialise detail/smooth=50 不算编辑 | external-rewrite case | 打开一次 Lightroom 就凭空改变降噪 detail |
| recolour carrier 编辑后 gains 删除，Temp/Exposure取外部值 | recolour case | 新颜色与旧 gains 叠加两次 |
| explicit zero 写为 `SharpenDetail=0`，untouched保留，非零编辑清标记并胜出 | explicit-zero case | 真 0 变 Lightroom default，或 40 仍被错误解释为 explicit zero |

Mutation 证据：

- `choose` 对 exposure 永远采用 payload 时，外部 `-0.5` 读成 `0.35`。
- colour field 强制采用 CRS 时，AutoShade-only field 变 `None`。
- 丢弃未配对的 current mask 时，`LR added` 从公开 names 列表消失。
- 删除 Lightroom materialised-default 规则时，detail 读成 50 而非 payload 的 0。
- 绕过 `recolour_carrier` 时，外部 Temp edit 仍带旧 gains。
- writer 的 `states` 忽略 `explicit_zero` 时，公开 XMP 不再含
  `SharpenDetail="0"`。
- 只破坏 explicit-zero reconciliation 或只破坏 clamp 过滤时 case仍绿，证明两层
  可独立兜底；同时破坏两层后，非零 edit 仍带旧 marker，case 红。

全部 mutation 均已恢复，`src/xmp/payload.rs`、`src/xmp/write.rs` 与
`src/recipe.rs` 无 diff。

### Raster materialization and claim cleanup

- Main cases：
  `rasters_are_placed_beside_the_develop_on_a_disclosing_read_and_never_on_a_silent_one`
  与 `a_claimed_name_whose_write_fails_is_released_not_left_half_written`
- 当前 cases：
  `disclosing_xmp_reads_place_rasters_without_overwriting_existing_files` 与
  `a_failed_raster_claim_releases_the_half_written_name`
- 状态：2 条 `rewritten`
- Gate：默认；claim failure 的一次性 fixture 仅在 Unix 编译运行

逐断言映射：

| main 断言族 | 当前执行者 | 裁决 |
| --- | --- | --- |
| silent read 不写 raster | public silent reader 后两个目标都不存在 | 保留 |
| 私有 silent restore 返回尚不存在的 anchored path | public silent reader 返回 develop sentinel | 改写为当前公开安全契约，不暴露不可读路径 |
| disclosing read 写出两份精确 bytes、recipe 指向它们、无 warning | public diagnostic reader + 实际文件/recipe/collector | 保留 |
| 相同文件再次读取幂等且安静 | 第二次 public diagnostic read | 保留 |
| 同名异内容时旧文件不变、新文件为 `-2`、bytes/recipe/warning一致 | public collision path | 保留 |
| claimed handle 正常写入 | 上述首次写与 collision 写的真实文件结果 | 改写，不直接调用 private helper |
| claimed handle 写失败后名称释放 | Unix 子进程用 `RLIMIT_FSIZE=0` 让 public collision 的 `create_new` 成功、write失败 | 保留真实错误路径；资源限制不污染父进程 |

Mutation 证据：

- public reader错误把 silent read也设为 materialize时，目标文件提前出现。
- collision 分支错误复用原名时，recipe仍指向 `tile-r3c3.png` 而非 `-2`。
- 删除写失败后的 `remove_file` 时，子进程留下零字节
  `tile-r3c3-2.png`，fixture与父 case依次红。

全部 mutation 均已恢复，`src/xmp/read.rs` 与 `src/xmp/payload.rs` 无 diff。

### XMP round trips and attribute forms

- Main file：`src/xmp/tests/round_trips_and_attribute_forms.rs`，19 条 cases
- 当前执行者：`tests/xmp_external_contract_behavior.rs` 的 15 条公开 cases，加上
  `tests/xmp_behavior.rs` 的 payload/composition/bitmap cases
- 状态：19 条 `rewritten`
- Gate：默认

逐 case 归属：

| main case | 当前行为执行者 |
| --- | --- |
| globals round trip / own As Shot tint | full global round-trip + foreign-WB case |
| parametric mask geometry/local controls/inversion | full local round-trip + native composition/inversion cases |
| bitmap only through payload | bitmap disclosure/payload case + photo-less inert-path case |
| foreign As Shot / identity curve | foreign-WB case |
| hostile XML values | hostile-text artifact/round-trip case |
| comments and whitespace | comment/attribute-whitespace import+merge case |
| readable corrections / preserve block | named mask import-loss case |
| recipe masks outrank foreign block | foreign-block preserve/replace case |
| attribute curve read/replace | attribute-form curve case |
| attribute mask group | same foreign-block case using `rdf:parseType` spelling |
| whitespace close tag | trailing-space public reader case |
| unreadable attribute curve | malformed-curve disclosure case |
| foreign camera-raw prefixes | public namespace refusal/diagnostic case |
| ancestor namespace | ancestor element-form merge case |
| ratings-only insert | same case's ratings arm |
| bounded XMP/numeric groups | scalar/group/resource-boundary case |
| named profile strength / unnamed foreign strength | two lens-profile strength cases |

关键断言映射：

- global fixture 精确覆盖 HSL、Color Grade、detail、crop、composite/channel curves、
  rationale/confidence；local fixture精确覆盖 range、amount、inversion和全部local sliders。
- foreign As Shot不导入相机 WB，Custom导入；identity master curve归空。
- XML attribute/text escaping、comment skipping、attribute whitespace、ancestor namespace、
  property-element close whitespace都通过公开 reader/merge验收。
- attribute-form曲线可读且只替换一次；越界或未闭合曲线不部分导入并具名披露。
- foreign mask block在recipe未改时逐字保留，recipe带新mask时替换且产生一条可见note。
- alias/rebind namespace拒绝merge并命名错误；ratings-only sidecar走insert且保留rating。
- 16 MiB+ 输入在 reader/merge/diagnostic 三门一致拒绝；标量clamp，compound group整组拒绝。
- lens strength只有在setup+name同时存在时按文件值生效；writer只有能命名真实profile时
  才把非100 strength交给Lightroom，否则具名loss。

Mutation 证据：

- foreign As Shot强制按Custom读取时，5150/+10错误成为用户编辑。
- master curve reader强制返回空时，attribute-form curve case越界失败。
- merge强制不preserve mask block时，无mask recipe删除用户Brush。
- namespace conflict merge gate旁路时，foreign alias被错误接受。
- oversize diagnostic gate旁路时，16 MiB+ 文档不再报告资源上限。
- reader无条件信任lens strength时，缺setup/name的50不再按Lightroom绘制为100。
- comment scanner不跳过 `<!--...-->` 时，伪Description劫持merge。
- attribute quote不escape时，公开 XMP不再含 `&quot;sky&quot;`。
- writer从不命名Adobe profile时，非100 strength没有LensDefaults/name。
- refusal counter强制为0时，公开 unsupported count与loss workflow红。
- close-tag matcher退回精确字符串时，`</crs:Exposure2012 >`读成0。

上述 mutation 均已恢复，`src/xmp/read.rs`、`src/xmp/merge.rs`、
`src/xmp/crs_source.rs`、`src/xmp/scan.rs`、`src/xmp/xml.rs`、
`src/xmp/read_masks.rs` 与 `src/recipe.rs` 无 diff。

### AI mask import and write-back

- Main cases：`ai_masks_and_frame_scope.rs` 前五条 AI import/base/gesture/write-back/zone
- 当前新增：
  `tests/xmp_external_contract_behavior.rs::external_ai_component_imports_beside_its_parametric_base`
  与 `external_ai_mask_rewrites_its_intent_with_fresh_identity_and_disclosure`
- 复用：
  `tests/domain_behavior.rs::an_external_ai_mask_preserves_its_reference_provenance_and_gesture`、
  XMP role/inversion cases，以及显式慢 gate
  `fit_workflow::a_fitted_zone_survives_xmp_as_the_same_delivered_pixels`
- 状态：5 条 `rewritten`

逐断言归属：

- Linear + AI 的外部 correction 保留Linear base，AI按Add component进入，subtype、
  reference point、provenance与`raster=None`精确可见，并产生`AiMaskRecomputed`。
- AI-only correction由现有payload-disabled public case验收为base；gesture count/dabs、
  model/origin provenance均由同一case执行。
- external AI重写重新发出Mask/Image、subtype/reference/version和provenance，不复用
  foreign SyncID，并通过公开loss channel披露alpha re-derivation。
- Select Sky zone的role、whole/geometry inversion home与net inversion由默认XMP cases
  执行；zone的真实像素交付由fit battery执行，不把内部fit attachment当测试入口。

Mutation 证据：

- classifier把`Mask/Image`重新标成geometry unusable时，Linear correction整体消失，
  component case在`0 != 1`处红。
- writer取消AI base routing时，rewritten artifact不再含`Mask/Image`，write-back
  case红。
- role name fallback、payload inversion home、native composition和fit delivered-pixel
  的既有mutation证据见前述对应段落。

mutation 已恢复，`src/xmp/read_corrections.rs`与`src/xmp/write.rs`无diff。

### AI mask vocabulary and scope

- Main cases：`ai_masks_and_frame_scope.rs` 的 unknown attribute、subtract pair、
  nested slider、range ownership、single-quoted attributes 五条
- 当前新增：`xmp_external_contract_behavior.rs` 的四条 parser cases
- 复用：`xmp_behavior.rs::edited_ai_component_modes_override_carried_blend_and_keep_plain_add_zero`
- 状态：5 条 `rewritten`

逐断言归属：

- AI mask只有在subtype属于0/1/2且属性名属于closed vocabulary时导入；未知名称和
  subtype 7都产生drop loss。
- native AI composition三态矩阵执行Subtract的`blend=1,value=0`以及round-trip。
- component上的`LocalExposure2012=9`既不能给correction赋值，也不能使它out-of-model；
  correction自己的contrast仍导入。
- 相同RangeMask在CorrectionMasks外为None，在component list内为Luminance。
- single-quotedAI attributes携带provenance并以canonical double quotes重写；single-quoted
  unknown attr仍拒绝。

Mutation 证据：

- unknown-attribute arm从`Err`改为忽略时，invalid AI correction错误导入。
- `correction_own_scope(seg)`退回whole-segment Scope时，nested slider令correction被拒绝，
  mask count从1变0。仅在parser内部改一个Scope仍绿，不计为证据。
- range parser追加whole-segment fallback时，component list外的RangeMask被错误附着。
- attribute lexer退回只认double quote时，single-quoted correction整体消失。
- Subtract matrix的既有native spelling mutation见Native XMP composition段落。

mutation 已恢复，`src/xmp/read_corrections.rs`、`src/xmp/read_masks.rs`与
`src/xmp/scan.rs`无diff。

### Declared frame scope through public geometry

- Main cases收敛：split two-Description fallback、mixed attribute/element spelling、
  split three-Description fallback
- 当前 cases：
  `rotated_mask_frame_fields_come_from_one_complete_description_in_both_spellings`
  与 `rotated_mask_frame_falls_back_across_incomplete_descriptions`
- 状态：3 条 `rewritten`
- `the_declared_frame_comes_from_one_description` 的完整Description断言由公开geometry
  case执行；同case附带的bare-fragment断言不保留。
- `the_frame_scope_falls_back_to_a_bare_fragment`标为`drop`：它的两条断言只直测私有
  `FrameAspect::from_xmp`。实际尝试把相同tiff fragments与真实mask block交给公共
  `xmp_to_recipe`，结果为0 masks，证明它不是公开输入契约；没有为测试开放API。

测试不公开或直调私有 frame parser。它由 public writer生成24° radial + 9504×6336
artifact、去除payload，再通过 public reader比较最终`MaskGeometry::Radial`：

- 6000 width decoy不能污染后一个完整Description；
- width attribute + length property element属于同一个frame；
- 没有完整pair时，width/length或width/length/orientation分散在多个Descriptions仍走
  documented fallback并保持同一geometry。

Mutation 证据：resolver无条件退回whole document时decoy改变radial bounds/angle；
resolver删除whole-document fallback时split artifact把24°读成0°并改变bounds。两条
均在公开geometry相等断言处红；mutation已恢复，`src/xmp/frame.rs`无diff。

### MaskBrushTable discovery refusals

- Main cases本批：fixed-point mapping、missing companion、malformed container、absent key
- 当前新增：
  `missing_mask_brush_companion_is_disclosed_as_unavailable`、
  `malformed_mask_brush_companion_is_disclosed_as_container_invalid`、
  `absent_mask_brush_key_is_disclosed_as_reference_mismatch`
- 复用：`domain_behavior::an_aggregate_brush_table_imports_its_compressed_strokes`
- 状态：4 条 `rewritten`；multi-table owner/order case仍`open`

所有错误都通过public photo-aware XMP reader和`Diag::Collector`验收：recipe不制造空
brush mask，用户可见line分别含`MaskBrushTableUnavailable`、`ContainerInvalid`、
`ReferenceMismatch`。合法synthetic ACR由现有case执行r/f/d token以及value/radius/
flow/center-weight映射。

Mutation 证据：

- sibling open failure误分类为ReferenceMismatch时missing case红；对“无photo path”
  分支的错误mutation仍绿，不计证据。
- established header shape误分类为Corrupt时malformed case红。
- directory miss误分类为Unavailable时absent-key case红。

mutation 已恢复，`src/xmp/brush_table.rs`无diff。

下一小批继续使用真实public reader + synthetic ACR objects：

| 故障 premise | 公开分类 |
| --- | --- |
| blob bytes与directory/XMP digest不一致 | `DigestMismatch` |
| envelope version从4改成5 | `EncodingUnsupported` |
| digest-valid envelope内Brotli无效 | `Corrupt` |
| 实际解压101 bytes、XMP声明102 | `LengthMismatch` |

四条case都同时断言recipe不制造brush mask。把这四个refusal的public `name()`统一错误
改成`WrongClass`时，四条case全部在各自分类断言处红，另三类不受影响；mutation已恢复，
`src/xmp/read_masks.rs`无diff。

payload token层的下一对synthetic fixture分别携带unknown binary `h` opcode和完整record
后的trailing bytes；public reader不制造mask，并分别披露`PayloadUnsupported`与
`PayloadInvalid`。把两者的public分类名统一改成`WrongClass`时恰好两条case红，其他七类
保持绿；mutation已恢复。

资源边界批继续固定四个公开拒绝：

- table word必须为1，否则`PayloadUnsupported`且detail命名table word；
- record count在分配前受256上限；
- d-count在token walk前受65,536上限；
- state token同样计入65,536总token上限，即使没有新增dab。

同时把三个数量上限放宽到1,000,000时，三条case分别失去自己的早拒绝detail，其中
token fixture甚至错误生成mask；移除table-word检查时对应detail消失。四条case均红，
mutation已恢复，`src/xmp/brush_table.rs`无diff。

独立表隔离case使用同一个synthetic ACR的good object与unknown-opcode object。公开读取
保留good table mask，也保留bad correction的inline Paint，bad table不贡献partial
record并单独披露`PayloadUnsupported`。把table error改成整个Aggregate直接失败时，
bad correction消失、mask count从2变1，case红；mutation已恢复，
`src/xmp/read_corrections.rs`无diff。

正向顺序与merge批固定：

- two-object ACR按XMP owner顺序产生First/Second masks，First内部A/B、Second内部C；
- unchanged table mask做无关exposure merge后，token与uncompressed byte count各保留一次，
  不把binary records再合成Mask/Paint。

parser返回前反转records时A/B次序case红；merge强制不preserve等价mask block时table attr
消失、merge case红。mutation已恢复，`src/xmp/brush_table.rs`与
`src/xmp/merge.rs`无diff。

residual/gesture批把binary table与inline Aggregate放在同一correction，并用另一条
AI correction携带`h` state token与`BrushGestureInterpretation=0`。公开reader保留
inline component和gesture，preserving merge保留解释属性。强制清空base之外components
时inline Aggregate丢失；跳过AI Gesture body时gesture丢失；两条mutation分别红并已
恢复。

真实P04 sample改为显式external gate：
`AUTOSHADE_MB_SAMPLE_ROOT=/path cargo test --release --test xmp_external_contract_behavior real_mask_brush_sample_matches_stage_three_ground_truth -- --ignored --exact`。
缺环境变量或fixture文件会失败，不再silent return。当前机器未提供fixture，因此只
验证该gate编译，未宣称54/4/18、3043 dab ground truth已在本轮运行。

### Global XMP representation and disclosure

- Main cases本批：representable atmosphere projection、global export losses、
  unmodelled imported globals（attribute/property forms）、XML equals whitespace
- 当前执行者：`xmp_external_contract_behavior.rs` 的5条global cases
- 状态：5 条 `rewritten`

产品断言：

- payload-free artifact明确写Exposure/WB/Tint/Saturation/master curve，不写RGB curves
  或bitmap mask；两个不同loss分别说明native Bitmap omission与payload raster missing。
- neutral/representable edits无global export loss，base curve与active lens profile具名。
- unmodelled disclosure只命名foreign globals，不把owned/pass-through/modelled key、
  attribute value中的`crs:`文本、nested Look参数或local mask keys误报。
- attribute、property-element与mixed spelling结果一致；tab/CR/LF围绕`=`仍保留完整key。

旧case对内部`RECIPE_CONTROLS` tier完整列表的精确断言不迁移；新增/重排registry row是
架构/data-shape，不是用户行为，public loss结果才是执行者。

Mutation 证据：

- writer漏写Saturation时artifact case红。
- global loss derivation过滤掉lens_profile时loss list case红。
- property-element walk关闭、attribute whitespace退回space-only时对应两条unmodelled
  cases各自红。

mutation已恢复，`src/xmp/write.rs`与`src/xmp/losses.rs`无diff。

### Camera-raw namespace scope

- Main cases本批：conflicting binding import/secondary channels、default namespace、
  unused/used nested rebind、foreign alias、closed-scope release
- 当前执行者：`xmp_external_contract_behavior.rs` 的4条namespace cases
- 状态：7 条 `rewritten`

同一冲突必须让public reader neutral、merge refuse，并让unsupported/import-loss/
unmodelled-global三个secondary channels保持empty，只由`unparsable_crs_numbers`给出
一次not-imported原因。scope规则另外验收：

- default-bound camera-raw namespace拒绝并命名DEFAULT；
- nested foreign rebind未使用时harmless，使用crs name时拒绝；
- island关闭后canonical ancestor重新生效；
- foreign `zzz` alias声明本身harmless，实际使用时拒绝并命名prefix。

Mutation 证据：

- reader单独绕过conflict gate时Exposure 2.5错误导入。
- 三个secondary channels同时绕过gate时drop count先从0变1，case红。
- scope退回flat declaration scan时unused island也拒绝，outer exposure从0.5变0。
- default/foreign-alias arms错误放行时对应两个public cases分别红。

mutation已恢复，`src/xmp/read.rs`、`src/xmp/read_masks.rs`、`src/xmp/losses.rs`与
`src/xmp/scan.rs`无diff。

namespace hardening批把另外4条私有scanner断言改写成公开后果：

- unused foreign Description被原样保留，merge另插canonical settings而不污染foreign scope；
- 1025层live namespace与单tag 257 declarations均拒绝merge并命名budget；
- misnamed close不能提前释放foreign binding，reader neutral、merge refuse、diagnostic
  命名`urn:foreign`。

Merge finder错误按`xmlns:crs`属性名采用foreign Description时canonical binding消失；
声明budget放宽到2000时deep/flat两case都失去拒绝diagnostic；close pop忽略tag name时
Exposure 2.0错误导入。三组mutation均红并已恢复，`src/xmp/scan.rs`无diff。

### XML item spelling and crop integrity

- Main cases本批：whitespace-spelled curve item、inconsistent crop disclosure、
  rotated left-over-right crop、attribute control characters
- 当前执行者：`xmp_external_contract_behavior.rs` 的4条公开读写cases
- 状态：4 条 `rewritten`

产品断言：

- 合法的`<rdf:li >... </rdf:li >`空白拼写仍公开为完整curve points。
- 缺坐标或不可能的crop不产生partial import，并明确命名`Crop`诊断；完整crop正常导入且
  无误报。
- writer产生的rotated crop即使编码后`CropLeft > CropRight`，reader仍恢复crop与angle，
  且不误报；把同一坐标的angle改为0后才拒绝并披露。
- mask name中的tab/newline/CR以XML character references写入，公开round-trip恢复原值。

Mutation 证据：

- close-tag匹配临时退回精确`</name>`时，合法空白curve由两个点变成空。
- 临时绕过structural crop disclosure时，invalid crop仍被拒绝但原因消失。
- 临时把所有`CropLeft > CropRight`判为invalid时，writer-produced rotated crop被误报。
- 临时把attribute控制字符原样写入时，character references消失且round-trip不再守约。

四组mutation均红并已恢复，`src/`无diff。

### Pathological XML partial audit

两条 main case 仍为 `open`，没有因为私有 `find_matching_close` 通过就宣称产品已保护。
当前新增的公开执行者只完成以下断言：

- comment、CDATA和processing instruction中的`</rdf:Description>`是文本；preserving
  merge保留construct及其后的foreign property，并写入新Exposure。
- 未终止comment拒绝preserving merge，不猜测文档边界。
- 80,000个连续comments或processing instructions的公开merge在5秒资源预算内完成并
  保留foreign property。

Mutation 证据：

- matching-close忽略construct边界时，合法文档被错误拒绝，公开construct case红。
- matching-close与span scanner同时吞掉未终止comment时，公开merge panic，refusal
  case红。
- construct landmark在每步重新扫描时，640,129字节comments从约40 ms退化到34.63 s，
  资源case红。

未完成的具体断言是deep nesting：把main的20,000层shape送入真实
`merge_recipe_into_xmp`后超过60秒；80,000层资源case也无法完成。它们仍是产品性能
缺口，不能由私有helper测试、较小fixture或当前三个cases勾销。所有临时mutation均已
恢复，`src/`无diff。

### Creative Look and scope fallback

- Main cases本批：nested creative Look isolation、owned-child scope fallback
- 当前执行者：`xmp_external_contract_behavior.rs` 的3条新增cases，加上既有global/local
  round-trip cases
- 状态：2 条 `rewritten`

逐断言结果：

- outer Description的Exposure正常导入；creative Look内部的Clarity、Vibrance和curve不
  变成global user edits，内部坏数字也不产生将被误导为用户设置的loss disclosure。
- 同名Clarity放在top-level attribute或property element时都导入为12；隔离只排除nested
  profile参数，不吞真正的用户编辑。
- 既有global/local round trips执行owned curve和named mask的公开保存结果。
- 不完整Description仍从whole-document fallback恢复top-level Exposure。main对私有
  `crs_scope_inner(...).is_none()`的中间态断言不迁移。

Mutation 证据：

- 关闭own-scope隔离时，global Clarity/Vibrance从`(0,0)`污染为`(50,35)`。
- reader忽略top-level Clarity时，attribute/element两种输入都从12退化为0。
- 关闭malformed-document fallback时，公开Exposure从0.5退化为0。

三组mutation均红并已恢复，`src/`无diff。

### Straighten carrier and XML controls

- Main case：`straighten_only_activates_crop_and_round_trips_to_no_crop`
- 当前执行者：
  `straighten_without_crop_uses_a_full_frame_carrier_and_round_trips_without_crop`、
  `forbidden_xml_controls_are_removed_from_public_mask_names`
- 状态：`rewritten`

原case的两个主张已经拆开。CRS-only straighten artifact写出`HasCrop=True`和
`CropAngle=-2.500000`，reader恢复2.5°但不把full-frame carrier变成真实crop。独立的
mask-name case证明NUL/BEL既不进入公开XMP，payload-free reader得到的名称也是`sky`。

临时让`engine_to_lr_crop`拒绝所有`crop=None`输入时，artifact首先丢失
`HasCrop=True`；临时允许任意control character进入XML时，NUL/BEL直接出现在document。
两条case分别红，mutation均已恢复，`src/`无diff。

### Real Lightroom radial probe gate

- Main case：`the_probe_sidecars_decode_to_their_measured_ellipses`
- 当前执行者：`real_lightroom_radials_round_trip_declared_geometry`
- 状态：`external-gate`

main case的真实价值是验证受控Lightroom exports与合成几何模型一致，不是默认套件里
无语料时silent return。当前gate显式`#[ignore]`，递归读取
`AUTOSHADE_LR_PROBE_FIXTURES`，并对每个radial sidecar验收：

- 声明frame有效且radial通过公开reader导入；
- Top/Left/Bottom/Right byte-identical round-trip，Angle误差小于`1e-4`；
- crop字段不被发明或改变，tiff frame声明数值保持一致；
- 配置缺失、目录/文件不可读、frame无效或0个radial均失败。

当前机器未提供真实probe目录，因此只确认该gate编译，并实测未设置环境变量时红灯；
不宣称真实fixtures已通过。显式启用方式：

```bash
AUTOSHADE_LR_PROBE_FIXTURES=/path/to/lr-experiment \
  cargo test --release --test xmp_external_contract_behavior \
  real_lightroom_radials_round_trip_declared_geometry -- --ignored --exact
```

默认mutation保护由现有radial geometry/frame/crop public cases执行；external gate只验证
这些合成前提对真实Lightroom artifacts的转录。

### Rotation loss angle payloads

- Main case：`the_rotation_loss_names_the_angle`
- 当前执行者：
  `export_rotation_loss_carries_a_signed_rounded_angle_or_plain_fallback`、
  `import_rotation_loss_carries_a_signed_angle_and_distinguishes_unrotated`
- 状态：`rewritten`

export typed loss分别验收`37.412506 -> Rotation(37)`、`-12 -> Rotation(-12)`与
`0.4 -> Rotation(0)`；import typed loss分别验收可读负角`-43.945287 -> -44`、不可读
angle的plain fallback `0`，以及literal zero不产生rotation loss。

临时把export显示舍入改为`ceil`时第一条结果变成38；临时抹掉import可读angle时
`-44`变成0。两个cases分别红，mutation均已恢复，`src/`无diff。

### Lightroom radial inversion pair

- Main case：`lightroom_spells_one_inversion_bit_twice`
- 当前执行者：`lightroom_radial_inversion_uses_one_bit_and_rewrites_its_complement`
- 状态：`rewritten`

公开case遍历Lightroom实际出现与冲突输入的四种`(Flipped, MaskInverted)`组合。reader
不把`Flipped`写入engine geometry flag，只以`MaskInverted`作为净反转；writer把净值
写入`MaskInverted`并把其补值写入`Flipped`，再次公开读取后净效果不变。

临时重新读取`Flipped`时，observed pair `(true,false)`从净false变成净true；临时让
writer写matching pair时，artifact从`(false,true)`退化为`(false,false)`。两组mutation
均红并已恢复，`src/`无diff。

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

### Jobs 并发顺序与资源计划

- Main cases：`src/jobs.rs` 中除 Windows peak probe 外的 8 条默认 case
- 当前入口：公开 `Sequencer::release`、`for_each_indexed`、`plan_with_peak`、
  `survey_peak_mb`
- 状态：8 条 `rewritten`；`probe_per_photo_peak_commit` 仍 `open`
- Gate：默认 `tests/jobs_behavior.rs`；两个 subprocess worker 自身明确 ignore

逐断言映射：

| main 断言组 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| completion 按 3/1/2/0 到达，stdout 仍按 0/1/2/3 | `sequencer_releases_completed_blocks_in_index_order` | 并行 batch 报告按完成顺序乱序，照片与诊断错位 |
| index 7 在 0..6 前到达时不输出 | `sequencer_holds_a_block_until_every_earlier_index_arrives` | 后续照片越过未完成照片提前发布 |
| 50% headroom / per-photo peak 得到 4/2/1 workers，未测机器不造 cap，较大 file peak 收紧 | `memory_budget_uses_half_the_headroom_and_the_declared_peak`；`an_unmeasurable_machine_does_not_invent_a_memory_cap`；`worker_plan_stays_within_the_requested_and_work_limits` | 并发 commit 超过机器预算，或测不到内存时被静默串行化 |
| request/work/memory 三者取最小；0 request/work 仍为 1 | `worker_plan_takes_the_smallest_of_request_work_and_memory`；`zero_request_and_zero_work_still_plan_one_worker` | 请求无 worker、空列表生成无效 pool，或 workers 超过照片数 |
| 内存真正降低 worker 时 note 同时命名 ask/headroom/peak；未降低时无 note | `a_memory_downgrade_names_the_request_headroom_and_peak`；`worker_plan_discloses_a_memory_limit_when_it_reduces_the_worker_count` | `--jobs` 看似失效却无解释，或无降级时产生虚假警告 |
| `u64::MAX` 邻域不 overflow 且至少一个 worker | `an_absurd_memory_reading_plans_without_overflowing` | 注入/异常内存读数导致 debug panic 或计划 0 |
| survey 从真实 baked headers 选择最大 peak 并命名文件；small/RAW/empty 保持 corpus default | `survey_uses_the_largest_baked_header_and_names_it`；`survey_keeps_the_corpus_default_when_no_header_exceeds_it` | 大 TIFF/PNG 被按平均/默认值低估，普通 run 又被噪声 note 污染 |
| parallel pool 结果按输入位置返回，每个 index 恰执行一次 | `parallel_pool_returns_results_at_their_input_indices`；`parallel_pool_visits_every_index_exactly_once` | 汇总值对应错照片、漏处理或重复付费调用 |
| one-worker 路径严格升序 | `one_worker_visits_indices_in_strict_order` | serial mode 不再等价于普通 for-loop |

Sequencer 的公开 stdout 通过当前 test binary 的 subprocess fixture 捕获：parent cases
默认执行，两个 worker cases 本身明确 ignore，避免默认套件把 fixture 当产品 case。
Pool tests 使用 atomics/Mutex，不用 sleep。Survey fixture 是带合法 IHDR/IDAT/IEND 的
header-only PNG，真实进入 `cheap_develop_peak_mb`，不分配声明尺寸对应的像素。

Mutation 证据：

- Sequencer 改成 arrival 即打印时，out-of-order 与 held-block 两个 parent case 均红。
- 内存预算比例改成 100%、0 request/work 不再 floor 1、cap note 被删除、饱和乘法改为
  普通乘法时，四个 planner case 分别在 worker 数、披露和 overflow 处变红。
- Survey 固定返回 corpus default 时 largest-header case 红；对所有输入伪报 oversized
  时 default/RAW/empty case 红。
- Pool 把结果写到下一个 slot、每个 body 调两次、单 worker 反转 index 时，
  positional、exactly-once、strict-order 三个 case 分别变红。

所有 mutation 均已恢复，`src/jobs.rs` 无 diff。

`src/jobs.rs::probe_per_photo_peak_commit` 保持 `open`。main probe 只验证探针 API 成功并
打印峰值，没有“峰值不得超过预算”的断言；按新原则它不是性能保护。后续必须把它
改成 Windows + 指定真实 RAW 的 external gate，并声明可执行的峰值阈值，才能标为
`external-gate`；不能因它是 slow/平台 case 就直接删除或宣称已保护。

### Stack alignment 的公开 affine / warp / merge 结果

- Main cases：`src/stack/align.rs` 中全部 8 条 case
- 当前入口：公开 `solve`、`Warp::apply`、`warp_rgb`、`coverage`
- 状态：8 条均为 `rewritten`
- Gate：默认 `tests/stack_align_behavior.rs`

逐断言映射：

| main 断言组 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| 两档曝光差下恢复 6/−4 px inverse translation，scale 仍约 1 | `a_two_stop_bracket_recovers_its_translation_without_inventing_scale` | bracket 因亮度差无法对齐，或为亮度差发明缩放 |
| 恢复 roll、1.2% breathing scale 与 translation | `affine_alignment_recovers_roll_scale_and_translation` | solver 退化成只能平移，focus/bracket 边缘重影 |
| covered pixels warp 回原 frame | `warping_back_reproduces_every_covered_pixel` | 解出的 warp 方向/采样错误 |
| coverage 标出真实 wedge，且大半 frame 仍 covered | `coverage_names_the_wedge_a_shift_pushes_outside_the_source` | merge 信任无来源像素或丢掉有效主体 |
| uncovered pixels 保持 moving frame 原值 | `an_uncovered_warp_pixel_keeps_the_value_it_had` | 边缘被填黑并污染 merge |
| split dx/dy residual planes 与独立 bilinear 采样 bit-for-bit 一致 | `a_split_residual_field_is_sampled_from_its_matching_planes` | residual 分量互换或 block-centre 坐标漂移 |
| 48/−32 px shift 在约五分之一 frame 越界时仍恢复 | `a_large_shift_is_recovered_even_with_a_fifth_of_the_frame_uncovered` | 大 wedge 的 clamp 内容把解拉回零 |
| registered burst 的 aligned merge 不劣于 unaligned noise reduction | `aligning_registered_frames_preserves_the_noise_stacks_improvement` | local aligner 从 grain 发明 motion，抵消 stack 降噪 |
| registered frames 的 uncovered <2%，每个 global warp corner travel <1 px | `registered_frames_stay_inside_their_view_and_near_identity` | 已对齐照片被推出自身视野或产生幽灵 global transform |
| aligned noise stack 仍移除 transient ghost | `registered_alignment_still_removes_the_transient_ghost` | 为求稳定而退化成直接保留异常 frame |
| local subject 相对 ground 移动 >8 px，flat blocks 全零 | `local_alignment_finds_a_moving_subject_and_refuses_flat_ground` | local pass 丢失 parallax 或从无纹理 sky 发明位移 |
| 192×144 frame 保留 9×6 grid，所有 subject blocks 可测，flat blocks 全零 | `a_small_frame_keeps_every_moving_subject_block_measurable` | coarse-level sample floor 永久杀死 finer-level 可测 block |

合成 scene 复用 main 的多尺度可辨识纹理公式，但所有采样、sRGB exposure 与 bilinear
oracle 均在 integration test 中独立实现；被测侧只调用公开 API。最初自造的单尺度
scene 无法恢复大位移，case 在 premise 上红；没有放宽阈值，而是回到 main 已证明可
辨识的 fixture 公式。

Mutation 证据：

- `solve` 固定返回 identity 时，两档 bracket、roll+scale、大位移三个 case 均红。
- `warp_rgb` 原样返回输入时 covered reconstruction 红；coverage 永真时 wedge case
  红；越界采样填黑时 keep-original case 红。
- `Warp::apply` 交换 dx/dy planes 时 split-field case 在首个坐标 bit 比较处红。
- `BLOCK_MARGIN` 设为 0 时 registered merge parity case 红；Noise merge 直接发布 ghost
  frame 时 ghost-removal case 红。
- 禁用 local pass 时 local-subject 与 small-grid cases 均红；把 coarse sample floor 从
  “跳过 level”改成“永久杀死 block”时 small-grid case 在 subject-block 断言处红。

所有 mutation 均已恢复，`src/stack/align.rs` 无 diff。

Surviving mutation 记录：只删除 Shi–Tomasi texture gate 时，flat-region 公开结果仍为
零；即使同时放宽 field-agreement，singular/后续拒绝仍维持产品行为。因此当前 case
保护“flat ground 不产生 residual”，不声称某一个内部 gate 必须存在。若未来删除这些
等价后备也让 flat residual 泄漏，本 case 会红。

### Stack merge 的公开成片结果

- Main cases：Composite 5 条与 Radiance 全部 5 条
- 当前入口：公开 `stack::merge::merge`
- 状态：10 条 `rewritten`；uncovered injection 仍 `open`
- Gate：默认 `tests/stack_merge_behavior.rs`

逐断言映射：

| main 断言组 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| 两个 frame 各自一半 sharp；Focus 输出两半 detail >99% sharp truth，seam 外像素误差 <0.02 | `a_focus_stack_reproduces_the_sharp_frame_on_both_sides` | Focus stack 退化成某一帧，或锐/虚混合导致两边都软 |
| 普通 exposure 的 shadow premise、+4 EV 的 highlight premise均成立；Fuse 输出两半 contrast >0.12 | `exposure_fusion_recovers_contrast_in_both_halves_of_a_bracket` | Fusion 只保住 bracket 的一半动态范围 |
| equal-detail 彩色/灰色对照保留至少 70% saturation | `exposure_fusion_prefers_colour_when_detail_is_equal` | Fusion 只看 detail/well-exposedness，洗掉颜色 |
| equal-detail midtone/pale 对照输出平均 level <0.6 | `exposure_fusion_prefers_midtones_when_detail_is_equal` | Fusion 不看 well-exposedness，漂向近白 frame |
| 单帧 grain premise成立，六帧 Noise merge 的 RMS 至少减半 | `a_noise_stack_halves_the_grain` | Noise stack 退化成 median/单帧，没有真实降噪 |
| transient ghost 区域最大误差 <0.03 | `a_noise_stack_removes_a_transient_ghost` | 普通平均把行人/物体残影留进成片 |
| Hdr/Fuse/Focus/Noise 均返回完整 frame 与每输入一项 exposure | `every_stack_kind_returns_a_full_frame_and_one_exposure_per_input` | enum kind 接到错误/缺失 arm 或丢失输入元数据 |
| 单帧与 ragged frame 被拒绝；store spelling round-trip | `a_stack_refuses_one_frame_and_mismatched_frame_sizes`；`stack_kind_store_spellings_round_trip` | 非 stack 输入被接受，或持久化 kind 无法恢复 |
| Reference window 已平白 clip；HDR 恢复 >0.05 structure、>1.5 EV headroom，同时 room 误差 <0.02 | `hdr_merge_restores_structure_above_white_and_preserves_the_room` | HDR 仅整体压暗，未恢复真实高光信息，或 shoulder 污染 midtones |
| Bright frame 只 clip red 的 premise；merged warm highlight 保持 0.30/0.15 线性色比 | `hdr_merge_preserves_the_colour_of_a_partly_clipped_highlight` | 各 channel 独立选 frame，把高光 hue 推偏 |
| Dark reading 除 exposure 后误差 >3× bright；merged RMS <1.4× bright | `hdr_merge_gives_more_weight_to_the_frame_that_collected_more_light` | 等权 bracket 把低光子数 frame 的放大噪声带入 midtones |
| 0→4 linear HDR ramp：headroom≈2 EV、knee 下 identity、top→white、单调、reference white<0.97 | `hdr_shoulder_preserves_low_values_reaches_white_and_stays_monotonic` | shoulder 硬 clip、压暗 midtones、折叠高光或把恢复藏在顶部极窄区间 |
| 0/−2/−4 EV bracket 恢复 bulk 1.9 与单个 8.0 glint，headroom 仍≈log2(1.9) | `one_hot_pixel_does_not_set_the_hdr_headroom` | 单个 hot/specular pixel 压低整张 HDR 成片 |

Mutation 证据：

- Focus sharpness decisiveness 从 cube 改成 linear 时，双侧 detail-energy case 红。
- Fuse arm 直接返回第一帧时 shadow-half contrast 断言红。
- 删除 saturation 权重时 colour case 从期望 0.122 降至 0.044 并红；删除
  well-exposedness 时 midtone case 升至 0.690 并红。
- Noise arm 返回第一帧时 RMS reduction 红；`GHOST_SPREADS=∞` 时独立 ghost case 在
  0.0848 最大误差处红。
- Focus arm 被临时拒绝时 every-kind case 红；移除 one-frame guard 时 invalid-input
  case 红。
- HDR arm 直接返回 reference 时 clipped-window structure case 红；trustworthiness 只读
  green channel 时 warm highlight 色比变成 0.402/0.201 并红；confidence 不随 exposure
  时 bright-frame noise case 红。
- Shoulder `stops` 退化为 identity 时 reference-white room 断言红；headroom percentile
  改为 maximum 时单 glint 把公开 headroom 从约 0.926 推到 3.000 EV并红。

所有 mutation 均已恢复，`src/stack/merge.rs` 与
`src/stack/merge/composite.rs` 无 diff。

`a_frame_the_warp_could_not_cover_is_not_averaged_in` 保持 `open`：公开 merge 不接收调用者
构造的 coverage，必须由真实 alignment 产生确定 wedge，不能复制 private cover 参数。

### Stack pyramid 的公开成片执行者

- Main cases：`src/stack/pyramid.rs` 全部 3 条
- 当前执行者：`tests/stack_merge_behavior.rs` 的 Focus/Fusion 成片 cases
- 状态：3 条均为 `rewritten`
- Gate：默认

main 直接断言 private `gaussian/laplacian/collapse/expand/blend_rgb`。当前审计不复制这些
helper；它用公开成片接管其产品后果：

| main 私有主张 | 当前公开执行者 | 产品故障 |
| --- | --- | --- |
| 分 band 后重建原 plane，coarsest brightness 不丢 | Focus 双侧 sharp-frame 像素 fidelity；Fusion colour delivery | merge 即使不改权重也变暗/失色 |
| expand 的 half-pixel grid 不让 ramp 滑动 | Focus seam 外最大像素误差 `<0.015` | pyramid level 错半个 coarse pixel，产生软 double edge |
| hard weight edge 按两边选择且无 step，任意权重先 normalize | Focus detail/fidelity；Fusion colour 与 bracket contrast | 单层 blend露 seam，或未归一权重造成亮度 stain |

Mutation 证据：

- 丢掉 Laplacian coarsest level 时 Focus seam 外误差升至 0.481，Fusion colour近乎归零。
- 去掉 expand 的 `−0.5` 时，公开 Focus seam 外 worst 从正确实现的 `0.01093` 升至
  `0.01952`；当前 `0.015` 成片阈值红。该阈值位于两者之间并保留约 0.004 平台余量。
- `depth` 固定为 1 时同一 Focus case 以 `0.01639` 红；跳过 weight normalization 时
  Focus detail、Fusion colour、bracket shadow contrast 三个公开结果均红。

所有 mutation 均已恢复，`src/stack/pyramid.rs` 无 diff。

### Store key 的 volume case 规则与 raster ownership

- Main cases：`src/store.rs` 的 5 条 case
- 当前入口：公开 `photo_key`、`volume_folds_case`、`OwnedRaster::claim` 与 zoned
  fallback workflow
- 状态：5 条均为 `rewritten`
- Gate：默认 `tests/store_behavior.rs`；Unicode key branch 仅在 Windows 编译执行；
  owned removal 由显式慢 Fit battery执行

逐断言映射：

| main 断言组 | 当前执行者 | 产品故障 |
| --- | --- | --- |
| 两个 case spelling 若在该 volume 指向同一对象就有同一 `photo_key`，否则不同 | `photo_keys_follow_the_volume_when_two_case_spellings_name_one_object` | 同一照片产生两个 develop/lock，或两个不同照片共享一个 |
| Windows non-ASCII stem 只折 ASCII 字母且字符数不扩张 | `a_windows_photo_key_folds_only_ascii_letters_in_a_non_ascii_stem`（`cfg(windows)`） | key 前缀变成 NTFS不能解析回原对象的 Unicode spelling |
| existing leaf 与 missing leaf 都按 volume truth回答 | `volume_case_detection_matches_the_filesystem_and_a_missing_leaf` | 输出覆盖/路径比较按 OS猜测而非实际 volume |
| 无可探测 entry 时回到 Windows/macOS fold、普通 Linux sensitive | `an_unprobeable_empty_directory_uses_the_platform_case_default` | 空目录首次写入使用不稳定 case规则 |
| 能创建两个 case spelling时 volume sensitive；第二个 collision时 folds | `two_case_spellings_fold_only_when_they_cannot_be_separate_files` | “存在另一个名字”被误当成“同一对象”，造成数据覆盖 |
| corpus photo 的 raster claim 只拥有 store placeholder，不拥有 corpus path | `claiming_a_raster_for_a_corpus_photo_owns_only_a_store_path` | fallback/reject清理删除校准语料 |
| owned placeholder 确实被 fallback remove | `an_unavailable_segmentation_sidecar_delivers_a_global_fit_and_releases_its_claimed_mask` | claimed临时 raster泄漏，或 ownership清理失效 |

Mutation 证据：

- `photo_key` 退回 lexical spelling时，当前 case-insensitive volume上的同一对象得到两个
  key，public case红。
- `volume_folds_case` 固定返回平台默认的反值时，live-volume、missing-leaf、
  empty-fallback、same-object四个 public对照中的三个直接红。
- `OwnedRaster::claim` 临时直接包装 source path时，corpus ownership case在
  `starts_with(corpus)` 断言处红。
- owned removal 的既有 mutation删除释放调用后，slow zoned fallback在真实 placeholder
  仍存在处红；证据登记于 `docs/BEHAVIOR_PROTECTION.md`。

所有 mutation 均已恢复，`src/store/key.rs` 与 `src/store/paths.rs` 无 diff。
Windows Unicode case已迁入但本轮 macOS未执行；最终跨平台 gate前不得把它表述成本地
验证结果。

#### Store 子模块的已闭合文件结果

`src/store/tests.rs` 截至本批有 51 条 `rewritten`、2 条 `lint`、1 条 `drop`：

| main case | 当前执行者 | 保留的产品契约 |
| --- | --- | --- |
| `one_file_spelled_two_ways_is_one_develop_key` | live-volume key case + Windows alias case | trailing dot/case/dotdot aliases共用 develop，不同照片不共用 |
| `a_staged_member_above_the_recovery_cap_refuses_the_save_whole` | `a_commit_with_an_oversized_member_leaves_no_partial_generation` | 错误命名 store limit，live members与 `.commit` stage均不留下 |
| `a_recipe_revision_names_the_bytes_and_absence_is_a_real_tag` | `a_recipe_revision_changes_when_the_saved_bytes_change` | absence可比较、同 bytes同 tag、不同 bytes不同 tag |
| `a_develop_snapshot_recovers_the_bak_before_choosing_a_recipe` | `a_develop_snapshot_recovers_an_orphan_recipe_before_reading_it` | snapshot先恢复 bak，再返回 central recipe与一致 pixel absence |
| `a_live_file_survives_recovery_and_keeps_its_retired_bak` | `a_live_recipe_is_not_replaced_by_an_orphan_backup` | newest live bytes胜出，retired bak不被误消费 |
| `photo_key_disambiguates_same_stem_different_folder` | `same_named_photos_in_different_folders_get_distinct_store_keys` | 平台 prefix正确、folder hash不同、重复调用稳定 |
| `a_nonblocking_develop_lock_reports_contention_and_recovers_after_drop` | thread + subprocess lock cases | held时 WouldBlock，释放后可重新取得 |
| `unknown_store_kinds_are_refused_without_rewriting_the_newer_record` | unknown pixel/variant cases | 两类 future kind均 unresolved且原 bytes不重写 |
| `a_develop_commit_lands_all_four_or_nothing` | first-generation / member-actions / unresolved-variant cases | 四成员同代落盘、下一代 write/clear/keep、stage清理、拒绝时全不落 |
| `read_text_capped_enforces_its_limit` | `a_store_read_past_its_byte_cap_is_refused` | at-cap通过，over-cap InvalidData命名limit，NotFound透传，坏UTF-8拒绝 |
| `an_empty_master_claim_is_refused_not_restored` | `an_empty_recorded_master_is_refused_without_falling_back_to_the_source` | 空 master不可读但 record仍存在，render-source公开入口拒绝静默回退 |
| `detach_rasters_frees_a_loaded_version_from_its_snapshot` | `a_detached_snapshot_raster_survives_deleting_its_source` | recipe改指独立同字节 raster，删除原 snapshot后仍可读 |
| `a_gallery_import_leaves_a_cleared_develop_alone` | `a_gallery_import_leaves_an_explicitly_cleared_develop_absent` | clear后的批量导入为0，central recipe不复活，legacy输入不被消费 |
| `orphan_bak_is_restored_on_read` | orphan restore + live-wins + unusable-record cases | orphan恢复、live owner胜出、absent与recorded-but-unusable可区分 |
| `a_cleared_develop_outranks_the_stale_copied_projection` | `a_clear_outranks_an_older_lightroom_sidecar_but_not_a_later_edit` | 旧sidecar不复活，clear之后的新Lightroom编辑仍胜出 |
| `clear_develop_leaves_no_resurrection_route` | clear live/backups + recovery + legacy suppression + idempotence cases | 当前develop与恢复路线清除、版本保留、legacy按照片抑制、重复clear无删除 |
| `an_unresolved_strip_refuses_save_and_clear_but_not_reads` | unresolved read/write/commit/clear cases | 读时区分Unresolved/Absent；所有破坏性保存拒绝且bytes不变，显式删除后恢复正常 |
| `clear_pixel_source_detaches_the_retired_bak_too` | `clearing_a_pixel_source_cannot_resurrect_the_previous_master` | 两代linkage公开写入后clear，recovery也不复活旧master，用户raster不删除 |
| `migrate_gate_treats_a_nondirectory_as_empty_not_failure` | `a_nondirectory_legacy_gallery_input_migrates_nothing` | 非目录输入公开返回迁移0且原bytes不变 |
| `has_develop_or_sidecar_counts_a_lightroom_sidecar_beside_the_raw` | RAW/baked badge + failed-clear cases | sidecar-only RAW可见，baked相邻XMP不算，clear进行中隐藏badge |
| `a_develop_snapshot_carries_the_xmp_layers_the_open_path_reads` | store/newer-LR/unreadable snapshot cases | store projection、新LR意图和unreadable披露均进入同一次公开snapshot |
| `variants_record_round_trips_relocatable_and_recovers_its_bak` | relocate + clear/recovery + future-format cases | 整个store搬迁后raster重锚；clear后不复活；future strip保持Unresolved |
| `variant_ids_and_names_are_additive_in_both_directions` | metadata preserve + Unicode cap + legacy None cases | 新旧metadata兼容、名称安全限长、旧记录不伪造id/name |
| `backup_snapshot_is_a_copy_and_freezes_rasters` | identical + frozen-raster + deleted-fingerprint cases | 相同写不增版本；快照独立；删除按内容与raster bytes判定且版本号烧掉 |
| `clearing_one_same_stem_photo_suppresses_but_never_unlinks_legacy_bytes` | same-stem clear/migrate case | A的clear只抑制A；B仍可见并迁移原bytes；A不复活 |
| `resolve_and_relativize_round_trip` | mask path round-trip case | bare路径锚定、缺失legacy引用不猜、in-store绝对路径重新变bare |
| `a_network_or_device_mask_reference_is_refused_on_every_platform` | unsafe + ordinary raster path cases | network/device引用禁用，普通本地store引用仍可搬迁 |
| `very_long_source_names_produce_portable_but_distinct_develop_keys` | long-key case | UTF-8/UTF-16组件均有界，截断前缀后仍靠16-hex身份区分 |
| `parsed_relative_store_paths_cannot_escape_the_develop_directory` | unsafe + ordinary raster path cases | `..`逃逸落到store sentinel，合法bare/legacy路径保留正常语义 |
| `depth_protect_and_layer_paths_are_refused_at_load_like_a_mask_path` | unsafe raster case | mask/depth/protect/layer走同一公开拒绝结果 |
| `depth_protect_and_layer_paths_round_trip_relative_like_a_mask_path` | additional raster round-trip case | 三类路径bare存储并重锚到原store |
| `a_version_snapshot_freezes_depth_protect_and_layer_rasters` | additional raster snapshot case | live改写后三类版本引用仍读取旧bytes |
| `a_rasters_family_is_its_name_without_the_claim_counter` | raster-family case | 扩展名、claim计数与版本前缀剥离，非计数后缀保留 |
| `move_no_clobber_adopts_owner_and_keeps_the_source` | central-owner legacy migration case | 已有central owner胜出且bytes不变，ambiguous legacy source保留 |
| `a_directory_junction_and_its_target_share_one_develop` | real directory-alias case | Unix symlink/Windows junction两种spelling共用公开key与develop |
| `the_verbatim_local_spelling_is_local_on_windows_and_refused_elsewhere` | cross-platform public resolver case | Windows保留verbatim local，其他平台禁用同一spelling |
| `the_develop_revision_folds_in_a_ranked_sidecar` | sidecar/store/unreadable revision cases | 胜出sidecar内容参与tag；旧sidecar不参与；不可读sidecar不可签名 |
| `a_pending_clear_completes_on_the_next_touch` | failed-clear recovery + later Lightroom case | 中途失败的clear在recovery完成，后续新Lightroom意图重新可见 |
| `a_zero_byte_live_claim_yields_to_the_surviving_bak` | zero-byte claim recovery case | 空claim不算save，survivor恢复并被公开snapshot读取 |
| `a_read_modify_write_of_the_strip_keeps_fields_this_build_never_heard_of` | active-kind read-modify-write case | 只改active kind，known identity/name与record/entry unknown fields全保留 |
| `list_edits_joins_the_strip_with_the_version_history` | public edit-state join case | active位置、background顺序、真实version、名称与provenance一次返回，phantom不出现 |
| `a_foreign_develop_write_restates_the_strips_active_card` | public commit restatement case | 只改active kind，active与background card其余字段保留；重复kind为Keep |
| `the_strip_member_keeps_its_hands_off_what_it_must` | four member-decision cases | Absent/Unresolved/Unknown为Keep；owner为exact Write或Clear |
| `a_pending_clear_outranks_the_sidecar_beside_the_photo` | public failed-clear badge/ranking case | pending intent同时压住badge与旧Lightroom sidecar排序 |
| `lightroom_sidecar_unreadable_is_disclosed_not_absent` | unreadable public reader/snapshot case | 返回Unreadable并命名UTF-8原因，不提供可用XMP文本 |
| `the_lightroom_hand_off_lands_beside_the_photo_and_never_clobbers_in_silence` | missing/first/refuse-overwrite handoff cases | 缺失时拒绝、首次精确交付、已有文件不静默覆盖、显式overwrite生效 |
| `a_pristine_card_without_a_pixel_record_hands_its_identity_to_the_fresh_master_card` | no-pixel-record generated-card case | edited active无继承身份，generated master card接过原id/name/origin |
| `migrate_legacy_moves_recipe_xmp_versions_and_rasters` | full bundle + deleted-version migration cases | recipe/XMP/version/raster迁入且legacy保留；幂等；deleted version不复活 |
| `fnv1a64_is_the_reference_function` | public photo-key suffix case | 三个canonical identity的16-hex suffix匹配独立FNV-1a reference |
| `deleting_a_version_that_does_not_exist_leaves_no_marker_and_burns_nothing` | missing-version delete/claim case | NotFound保持，列表为空，后续claim仍从v1开始 |
| `a_killed_version_delete_resumes_and_its_number_stays_burned` | public delete-fault/recovery case | 半删除version立即隐藏，recovery扫完，后续claim从v2且文件存在 |

架构归属：

| main case | lint执行者 | 覆盖的代码约束 |
| --- | --- | --- |
| `the_version_meta_sidecar_keeps_the_non_generational_discipline` | `store-advisory-metadata-lifecycle` | advisory文件名固定；只走direct durable write；不得进入adoption exclusion、orphan recovery、clear或commit generation |
| `the_store_directory_name_and_the_rename_adoption_agree_per_platform` | `store-root-platform-contract` | macOS显示名、其他平台小写名、Application Support父目录、macOS禁用pre-rename adoption |

该 rule 随 `scripts/check_architecture.py --self-test` 运行：坏夹具同时把 metadata 接入
generational publisher与四个禁止scope，必须报
`store-advisory-metadata-lifecycle`；好夹具使用 direct publisher 且四个scope不引用
metadata，必须通过。这里的红灯回答“代码允许怎么写”，所以没有复制 main 的私有
helper/list断言到行为测试。

`store-root-platform-contract` 的坏夹具分别把两个平台目录名对调、无条件开启 adoption、
并把 macOS 根移出 `Library/Application Support`；self-test 必须报该 rule。好夹具同时
满足四项并通过。旧 case 的三个源码 `contains` 断言因此删除，平台配置仍有实际执行者。

删除归属：

| main case | 删除原因 |
| --- | --- |
| `the_tombstone_fixture_cleans_up_through_a_panic` | 只验证 main 测试自己定义的 `LegacyTombstoneFixture::Drop` 能清理合成残留；没有生产入口、产品状态或用户可见故障语义 |

这不是因为 case 慢或难迁移；它的唯一 mutation 是删掉测试夹具的清理代码，产品二进制
完全不变。测试政策本身禁止用这种“测试保护测试辅助实现”的 case 充当行为保护。

`clear_develop_leaves_no_resurrection_route` 有一组 main 断言不能原样保留：
旧 case 要求物理删除 stem-only legacy recipe/XMP；当前产品契约改为写入按照片
tombstone、让这些共享 bytes 对该照片不可见但不删除。直接删除可能同时删除另一目录中
同 stem 照片仍需迁移的唯一旧数据。因此这两条断言归为“已被当前公开行为取代”，不是
缺失测试；当前 case 同时断言 `has_develop == false` 与 legacy bytes 原样保留。

本批新增 case 的逐断言组映射：

| main 断言组 | 当前执行者 | 失败所代表的产品故障 |
| --- | --- | --- |
| 空 master读取为None，但record仍计为存在 | empty recorded-master case | 导出接受空文件，或把已损坏retouch误当成从未保存而静默回退 |
| detach后路径不同、bytes相同、源删后副本仍在 | detached snapshot-raster case | 加载版本仍引用快照所有物，删版本后当前编辑丢mask |
| clear后gallery import为0、central缺失、legacy输入仍在 | cleared gallery-import case | 用户明确清除后，批量导入把旧编辑复活或消费旧证据 |
| orphan恢复并消费；live不被旧copy覆盖；unusable record不等于absent | orphan/live/unknown-pixel/empty-snapshot cases | 崩溃后的最新保存丢失、并发新保存被覆盖，或坏master静默降级 |
| 旧Lightroom sidecar输给clear，后写sidecar胜出 | clear/Lightroom ordering case | Reset+Save立即复活旧编辑，或之后的真实Lightroom编辑永远无法恢复 |
| live成员消失、version保留、恢复后仍为空、重复clear报告no-op | clear result/recovery/idempotence cases | 清除半成品复活、误删历史版本，或UI谎报删除了不存在的develop |
| legacy文件对该照片不可见但原bytes保留 | legacy suppression case | 同stem照片共享的旧数据被误删，或被清除照片仍读取旧数据 |
| unresolved strip可读为Unresolved，但write/commit/clear均拒绝且bytes不变 | unresolved variant cases | 普通保存或清除销毁当前版本无法理解的背景卡 |
| public pixel clear后再recovery仍无linkage，两份master原文件保留 | pixel-source clear case | 旧retouch在下次打开复活，或detach误删用户raster |
| 非目录legacy gallery输入返回0并保持原文件 | nondirectory migration case | UI虚报迁移，或把普通文件当目录改写 |
| RAW相邻sidecar计入badge；baked不计；失败clear进行中仍压住badge | sidecar badge cases | 对Lightroom-only RAW重复付费分析，或给PNG错误badge，或clear中途复活旧意图 |
| store XMP、新er Lightroom XMP、unreadable sidecar分别进入snapshot对应字段 | snapshot XMP cases | batch和open对同一照片选择不同develop，或把不可读工作静默当无编辑 |
| 搬迁data root后origin与mask路径指向新store，clear后recovery仍Absent | relocated/cleared variant cases | store迁移后背景卡丢raster，或明确清除的旧strip复活 |
| unknown字段与已知id/name保留；长Unicode名按公开字节上限截断；旧记录仍None | variant metadata cases | 新版读取旧strip伪造身份，旧版字段在修改时丢失，或恶意名称撑爆UI/store |
| 相同raster recipe不增版本；快照复制并冻结bytes；删除指纹区分新旧bytes且烧号 | snapshot/fingerprint cases | 自动版本膨胀、live改写污染历史、删除后错误复活或编号复用 |
| 同stem照片A clear后A不可见、B仍可见并迁移、共享legacy bytes未被提前删除 | same-stem clear/migrate case | 清除一张照片误删或全局屏蔽另一张同名照片的唯一旧编辑 |
| raster路径相对化/重锚、unsafe引用统一禁用、版本冻结bytes独立 | six path behavior cases | store搬迁丢raster、打开恶意store访问外部路径、历史版本随live改变 |
| 超长key有界且不同源仍不同；family只忽略真实计数 | long-key / raster-family cases | 文件系统拒绝过长develop目录、两个源静默共用key、rerun追加重复mask |
| central owner存在时migration报告未迁移、owner与legacy source bytes都不变 | central-owner migration case | 旧legacy bytes覆盖新保存，或migration误删无法证明归属的共享源 |
| 真实目录alias与target的 `photo_key`、`develop_dir`分别相同 | directory-alias case | 同一照片经不同spelling生成两个develop与两把锁，后续保存分叉 |
| verbatim local raster path在Windows保留、其他平台落到store sentinel | cross-platform resolver case | Windows误禁真实本地raster，或其他平台把无效spelling当成本地路径 |
| sidecar-only内容变化重签；store胜出时保持recipe tag；unreadable返回None | develop-revision cases | stale If-Match覆盖Lightroom新编辑，或不可读工作被当成可安全覆盖 |
| clear中途失败后recovery清空develop，之后的新Lightroom sidecar重新计入badge | pending-clear recovery case | pending intent永久遮住后续编辑，或半清除文件继续被读取 |
| 0-byte live claim + survivor恢复成真实bytes并进入snapshot | zero-byte recovery case | 双崩溃窗口把空claim当成最新保存，唯一可恢复recipe永久隐藏 |
| active kind改写后两层unknown fields与known identity/name均原样保留 | variant read-modify-write case | 旧版CLI保存静默截断新版GUI写入的strip metadata |
| edit-state列表按记录位置排列cards，随后只列真实version并携带metadata | edit-state join case | GUI错选active卡、显示已删除phantom版本、版本名称/来源丢失 |
| foreign commit后active kind更新，card/background完整，重复同kind不重写 | variant restatement case | CLI写入新develop后strip仍描述旧卡，或重写时丢背景卡/身份 |
| 四类caller分别得到Keep、exact Write或Clear | variant member decision cases | 无记录被凭空制造、陌生数据被清除、owner交付被忽略、未知caller误改strip |
| clear中途失败时badge为false且`lightroom_sidecar`为None | pending-clear ordering case | 清除尚未完成时旧sidecar立即复活为最新编辑 |
| 非UTF-8 sidecar返回Unreadable原因，snapshot披露且无lr_xmp | unreadable sidecar case | 损坏或陌生编码被静默折成“没有编辑” |
| missing projection无文件且命名remedy；首次交付bytes/path/RAW；拒绝与overwrite | Lightroom handoff cases | 空claim冒充交付、覆盖用户sidecar、错误路径或改坏源照片 |
| 无pixel record的generated active被编辑时，身份移到fresh master card | generated-card identity case | 版本指向的card身份与用户命名在web/batch编辑后消失 |
| legacy完整bundle迁入central且源bytes保留，删除version后不从retained legacy复活 | legacy migration cases | 迁移丢mask/version、破坏旧数据，或用户删除的历史版本再次出现 |
| 三个真实photo key suffix匹配published FNV-1a identity | public key compatibility case | hash漂移导致所有既有develop目录、版本与锁对用户“消失” |
| 删除不存在v7返回NotFound，后续claim仍为v1 | missing-version delete case | stale UI操作留下事务残留并烧掉从未存在的版本号 |
| version delete因frozen-raster目录中断后隐藏；recovery删除recipe；claim为v2 | interrupted version-delete case | 半删版本残留可加载文件，或已删编号被重新发放 |

两条 Lightroom version case 仍是具体 `open`，不是泛化缺口：

| main case | 已执行公开行为 | 尚无执行者的断言 |
| --- | --- | --- |
| `a_newer_lightroom_sidecar_is_snapshotted_before_a_programmatic_write` | store先成为v1、newer Lightroom使返回/列表到v2、重复不增、源sidecar未改 | v2保存的lossless XMP bytes与原sidecar逐字节相同 |
| `a_sidecar_only_develop_is_snapshotted_instead_of_reported_as_nothing` | 非中性sidecar-only得到v1；中性sidecar无版本 | v1保存的lossless XMP bytes与原sidecar逐字节相同 |

原因不是“不想测文件”：`version_target`公开recipe版本，但没有公开入口读取版本携带的
lossless XMP witness；直接拼 `v<n>.<stem>.xmp` 会把测试锁到私有布局。直到真实
Load Version工作流或公开版本读取入口能观察这份bytes，两条 case必须保持 `open`。

新增 mutation 证据：

- 取消 staged-member cap时 oversized commit错误变为 `Ok`，case在 `unwrap_err` 处红。
- revision固定为一个 tag时 changed-bytes 断言红；snapshot跳过 bak recovery时 recipe
  变 `None` 并红。
- `photo_key` 只用 stem、不含folder identity时 same-stem case红。
- unknown pixel kind按 `inplace` 接受、unknown variant kind无条件认作 known时，两条
  refusal cases分别红。
- recovery删除非零 live再恢复 bak时，live-wins case读到 older backup并红。
- XMP write no-op、member Clear no-op、绕过 unresolved-strip gate时，commit cases分别在
  XMP缺失、pixels仍存在、错误变 `Ok` 处红。
- capped reader把 `>` 改成 `>=` 时，正好8-byte输入被错误拒绝并红。
- detach成功后不更新recipe path时，case在“仍指向snapshot”处红。
- 跳过0-byte master gate时，empty-master case在公开读取返回Some处红；把
  `has_pixel_source`错误收窄成“必须可读”时，unknown-kind case在record存在断言处红。
- gallery import忽略clear tombstone时，迁移数由0变1并红。
- Lightroom排序忽略clear时间时旧sidecar复活并红；反过来让clear永远胜出时，
  post-clear新编辑无法恢复并红。
- clear不删除retired variants成员时，下一次公开recovery恢复strip并红。
- clear漏写legacy抑制时 `has_develop`仍为true；改回物理删除legacy bytes时，
  保留共享旧数据的断言红。
- `ClearOutcome.removed`无条件报true时，空develop幂等case红。
- 绕过 `clear_variants` 的 unresolved gate时，foreign strip被删除，clear-refusal case红。
- `clear_pixel_source`只删live linkage时，公开recovery复活前一代master并红。
- 非目录gallery输入错误返回 `photos.len()` 时，迁移数由0变1并红。
- 去掉 badge 的 RAW类型门时PNG错误计为develop；去掉 pending-clear门时失败clear后的RAW
  badge复活，两条case分别红。
- snapshot不填 `store_xmp`、不填 newer `lr_xmp`、不填 `lr_unreadable` 时，三条对应
  case分别在公开字段上红。
- 关闭variant origin/mask相对化后，真实data-root搬迁返回旧路径并红。
- `clear_variants`不删除retired generation时，公开recovery恢复旧strip并红。
- `capped_name`原样返回超长Unicode名称时，公开字节上限断言红。
- 写variant时给None字段伪造id/name，legacy round-trip在四个None断言中的第一个红。
- 完全忽略 Lightroom intent时，newer-sidecar case只返回v1、sidecar-only case返回None。
- 移除neutral sidecar gate时，中性sidecar错误创建v1并红；移除XMP版本去重时重复backup
  创建v3并红。
- snapshot不改写到frozen raster时，live raster改写后历史版本读到new bytes并红。
- 删除指纹只比较recipe结构、不比较raster bytes时，改变过raster的保存被错误抑制；
  忽略deleted registry high-water mark时，v1编号被复用。两次分别红。
- legacy tombstone错误按stem全局共享时，清除A后B的 `has_develop` 变false并红。
- key组件上限取消时long-key超240 bytes并红；相对化变no-op时mask与
  depth/protect/layer两个round-trip case均红。
- `raster_paths_mut`退回只遍历mask时，additional raster拒绝、round-trip与snapshot
  case分别红；`snapshot_rasters`只改引用不复制bytes时snapshot case也单独红。
- network/device lexical gate旁路时UNC mask不再落sentinel；containment无条件join时
  `../../outside.png` layer不再落sentinel。两次分别红。
- `mask_family`不剥claim/version计数时family case红。
- 仅移除migration早期owner检查仍绿、仅改坏未走到的non-recipe move也仍绿，证明两层
  防线各自冗余；同时移除owner检查并把实际recipe publish改成replace时，公开case在
  “未迁移”断言处红。该故障反例对应真实产品覆盖，不锁调用顺序。
- `photo_key`跳过canonical identity、直接hash调用者spelling时，alias与target得到不同
  key，directory-alias case在第一个公开相等断言处红。
- 非Windows旁路network/device spelling gate时，verbatim path被拼到develop目录而非
  sentinel，cross-platform case红；Windows arm随平台gate编译执行。
- 胜出sidecar不参与revision、旧sidecar错误参与revision、Unreadable返回base tag三种
  mutation分别让三条develop-revision case在各自公开结果处红。
- pending clear recovery直接返回不执行sweep时，后来Lightroom sidecar仍不计入badge并红。
- recovery把0-byte live当有效save时，survivor不恢复，zero-byte case在bytes断言处红。
- `variants_record_bytes`丢弃record/entry capture-all map时，read-modify-write case在
  record-level unknown field断言处红。
- `list_edits`把active card固定追加到末尾时，edit-state case在首项身份处红；不join
  version metadata时，同一case在版本名称断言处红。
- restatement时清空active id/background，case在identity断言处红；移除already-true
  的Keep快捷路时，case在最后的幂等断言处红。
- 将Absent改Write、Unresolved改Clear、owner改Keep、Unknown改Clear时，四条独立case
  分别在自己的member断言处红。
- newest-intent排序只看completed clear、不看pending clear时，badge断言仍绿但
  `lightroom_sidecar == None`断言红，证明两层各自执行。
- sidecar reader把Unreadable折成None时，snapshot disclosure断言红。
- missing projection错误丢掉workflow remedy时missing case红；首次handoff在claim后不发布
  bytes却返回成功时，first-handoff case在完整bytes断言处红。
- fresh generated master card重新使用`id: None, name: None`时，identity case在原id断言处红。
- raster prefix永不匹配时bundle case无法读取central raster并红；忽略deleted-version
  registry时第二次gallery migration返回1并复活v2，deletion case红。
- 生产FNV offset改变1时，首个public photo-key suffix断言红。
- 删除missing-version existence probe时，NotFound虽仍返回，但后续claim变v8，case红。
- orphan recovery跳过pending version deletes时，列表仍隐藏但v1 recipe未删除，interrupted
  delete case在文件结果处红。
- lock bypass的 thread/process mutation证据已登记于
  `docs/BEHAVIOR_PROTECTION.md`。

以下相邻 main cases继续 `open`，因为仍有断言未执行：组合式
`a_half_applied_commit_finishes_instead_of_tearing`、
`a_pending_clear_outranks_the_sidecar_beside_the_photo`，以及需要真实 crash
marker/subprocess fault injection 的 version-delete恢复。不能用本批的局部相似行为
提前勾销。

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

### XMP sidecar pairing

- Main cases：`src/xmp_pair.rs` 的 extension、mirror、flat、sibling、priority、absence
  与 missing-directory 7 条
- 当前执行者：`tests/xmp_pair_behavior.rs`
- 状态：7 条 `rewritten`
- Gate：默认

测试建立真实 library/sidecar 目录树，逐项断言公开 `XmpPairing::find` 返回的路径或
`None`。Mirror > flat > sibling 的优先级在同一 case 中通过依次移除上一级候选验证，
不是读取实现列表。Extension matcher改成严格小写时 mixed-case case红；搜索目录顺序
反转时 priority case返回sibling并红。两次mutation均已恢复。

`the_exact_lowercase_spelling_outranks_every_other_casing` 仍为具体 `open`：当前卷不能同时
创建 `shot.xmp` 与 `shot.XMP` 两个文件，无法建立竞争 premise；在没有大小写敏感卷
gate前不能用单一现存文件冒充该优先级已执行。

### Native XMP composition

- Main cases：`combine_spelling_matches_all_census_rows_and_both_inversions`、
  `mixed_component_order_and_whole_inversion_survive_native_export`、
  `component_inversion_is_sparse_and_legacy_components_keep_their_bytes`、
  `stale_radial_inversion_metadata_cannot_reverse_a_linear_feather` 与
  `component_radial_rotation_uses_the_base_projection_and_loss_gate`、完整composition矩阵、
  census-shaped外部Intersect导入、
  AI/Brush/complement三条，以及 namespace/portrait role 两条
- 当前 cases：`tests/xmp_behavior.rs::native_xmp_spells_every_combine_mode_and_component_inversion`
  与 `native_xmp_preserves_mixed_component_order_in_both_whole_inversions`
- 状态：12 条 `rewritten`
- Gate：默认

第一条对三种 mode × 两种 component inversion 逐一读取交付 XML 最后一个 mask 的
BlendMode/Inverted/Value，并验证 public import 还原 editor intent。第二条导出 mixed
Subtract/Add/Intersect components 后删除所有 `ash:*` editor
metadata，再从纯 CRS 文档读取；两种 whole inversion 下分别断言组件顺序/模式与最终
coverage。额外对非 whole-inverted 文件断言两个 `MaskBlendMode="1"`，固定 Lightroom
对 Subtract 和 Intersect 的公开拼写。

最初只做 round-trip 时，把 Subtract writer/reader 一起改成 Add 后测试仍绿，证明同源
读写会共漂。加入交付 XML 断言后同一 mutant 在 mode-1 count 处红；恢复后通过。
另一个 case 直接验收 `MaskComponent` 的公开 JSON：false 不新增 key、true 明确写出；
移除 sparse serde gate时 legacy JSON 断言红。
Stale radial metadata被无条件应用到linear时，公开 inversion断言红；component radial
忽略可用frame时，framed export错误产生Rotation loss并红。
Root namespace declaration被忽略时ZoneSky读成Custom；未知role错误降级为Face时
portrait case红。
取消plain Add zero保留时AI与Brush case的value均变1；whole complement不把Subtract
投影为Add时，Add-one case读回 `(blend=1,value=0)` 并红。
Composition矩阵覆盖8组Linear/Radial/AI组合；破坏whole-inverted Intersect的
De Morgan投影时，带intent读取仍可能正确，但CRS-only逐像素coverage断言红。
Photo-less public read携带6个mask/depth/protect/layer引用，全部变成绝对inert sentinel；
跳过统一disable入口时该case红。
把同一photo-less develop merge回base时，两边按相同禁用规则比较，原mask block保留且
无替换note；把base比较错误改成空masks时该case红。
7 MiB raster超过payload预算时不嵌入、loss命名mask、XMP保持小且recipe仍完整；把预算
提高到8 MiB时raster被嵌入、loss消失，资源case红。
17 MiB高压缩recipe带正确CRC时披露too large并保留CRS fallback；解压读取只取到
ceiling、不读取额外一字节时错误变checksum mismatch，case红。
两个mask共享同一不可嵌入raster时，每个用户可见mask各得一条具名loss；改成全局只报
第一条RasterNotEmbedded时第二个名称消失，case红。
纯Lightroom-shaped fixture把`blend=1,inverted=true`读为Intersect；改成Subtract时
外部导入case直接红。
其余 composition cases 继续逐项审计，不由这一条自动勾销。

`r35_export_truth_for_a_scratch_recipe_names_every_remaining_loss` 不再保留为依赖工作树
路径的 ignored lane。默认 case
`xmp_export_preserves_spellable_masks_and_names_each_omitted_bitmap` 用合成 recipe 验收：

- spellable mask 的 role、whole inversion、component order/mode/inversion 精确读回；
- payload 仍恢复三条完整 masks，不再复刻 main 时代“只返回 CRS mask”的中间态；
- enabled bitmap 得到唯一具名 `Bitmap` loss，disabled bitmap只得到 `Disabled`；
- native mask 不被误报 Bitmap/flatten/rotation。

删除 enabled bitmap 的 loss emission 时，recipe 因 payload 仍完整，但 `tile` disclosure
消失，case 在具名 loss 断言处红；mutation 已恢复，`src/xmp/write.rs` 无 diff。

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
| `tests/xmp_behavior.rs` | 40 | payload/namespace round-trip、recipe/raster ceilings与共享loss、photo-less safety/merge、native composition/bitmap disclosure、role fallback/intent、external edit reconciliation、raster materialization/cleanup、portable raster paths、Lightroom base reconciliation、Lightroom fixture gates、coverage | XMP、Render masks | 未提供 fixture 时的真实 Adobe rewrite gate |
| `tests/xmp_external_contract_behavior.rs` | 70 | global/local round-trip、global representation/disclosure、foreign WB/XML、attribute forms/control-character round-trip、curve item spelling、crop integrity/straighten carrier、pathological XML correctness/resource bounds、creative Look isolation/scope fallback、mask merge、namespace scope/refusal/hardening、resource bounds、profile strength、AI component/write-back/vocabulary/scope、declared-frame geometry、rotation loss/inversion payloads、MaskBrushTable refusals/budgets/isolation/order/merge/survivors、Lightroom radial/P04 external gates | XMP public format/compatibility | deep-nesting public merge remains open; real Lightroom/P04 gates require external fixtures |
| `tests/xmp_pair_behavior.rs` | 7 | extension case、mirror/flat/sibling discovery、优先级、absence fallback | XMP pairing | 大小写敏感卷上的同stem双扩展名竞争 |
| `tests/jobs_behavior.rs` | 15 | Sequencer stdout、pool positional/exactly-once、memory/survey budgets | Batch、Eval、资源边界 | Windows real-RAW peak threshold gate |
| `tests/stack_align_behavior.rs` | 12 | affine、warp/coverage、registered merge parity、local parallax、small-grid survival | Stack alignment、Noise merge | 真实相机 burst/focus sweep 外部语料 |
| `tests/stack_merge_behavior.rs` | 13 | focus、fusion weights/pyramid、noise/ghost、HDR structure/colour/noise/shoulder/headroom、front door | Stack merge、Pyramid | coverage injection |
| `tests/store_behavior.rs` | 102 source / 100 macOS | stable key identity/alias/bounds、atomic commit、locks、crash/clear/version-delete recovery、revisions、variant identity/member/edit-state joins、full legacy migration/suppression、XMP ordering/handoff、cross-platform safe raster paths、version snapshot/delete semantics、ownership | Store | Windows key aliases、remaining adoption crash races、lossless version XMP public read |
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
