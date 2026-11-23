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
- 当前工作树：384 cases，约 1,048 个断言宏，20 个显式 ignore。
- 名称完全相同的 case 只有 1 个；不能按名称自动宣称其余 case 已覆盖。

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
