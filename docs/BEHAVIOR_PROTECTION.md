# 程序行为保护方法

状态：当前生效

目标：提高 AutoShade 的行为回归检出强度，而不是提高覆盖率数字。覆盖率只能帮助发现未触达代码，不能用于考核、排名或判断测试是否有效。

## 方法比较

| 方法 | 最适合保护什么 | 不适合替代什么 | 当前落地 |
| --- | --- | --- | --- |
| 行为测试 | 公开入口、文件结果、错误文案、UI 可见状态、资源和部分失败 | 不能证明所有输入空间；不能替代 mutation 证据 | `tests/*_behavior.rs`、`src/bin/gui/tests.rs` |
| 性质测试 | 纯函数的代数性质、边界不变量、变换可逆性、计划约束 | 不能单独保护文件系统和用户工作流 | `tests/properties.rs` |
| 故障注入 | 写失败、坏 sidecar、覆盖风险、部分提交、回退路径 | 不能证明正常路径行为正确 | Store/XMP 失败用例 |
| mutation testing | 测试是否能发现真实行为改动 | 不能作为覆盖率替代品；不适合每次全仓运行 | `scripts/mutation_gate.py` + 定向 `cargo-mutants` |
| 编译器、类型、clippy、lint | 层依赖、允许导入、禁用 API、文件边界 | 不能判断用户可见行为是否正确 | `cargo clippy`、现有 gate |

选择顺序：

1. 能用公开行为描述的风险，先写行为测试。
2. 有代数或边界不变量的纯逻辑，增加性质测试。
3. 失败后可能留下半成品或错误文件的路径，增加故障注入。
4. 改动触及生产行为时，用定向 mutation 证明测试会变红。
5. 架构约束交给编译器、类型、可见性或 lint，不写成源码扫描测试。

速度不是入选条件。测试快只说明它可以放在默认 gate，不能说明它值得保留；测试慢只说明它需要显式 gate，不能说明它可以不测。

## 性质测试的边界

性质测试不是“随机跑几次”。每个性质必须满足：

- 从公开 API 进入。
- 前提明确，例如只测试单位方格内部点，避免把合法的裁剪外点误判为失败。
- 断言描述不变量，而不是当前实现细节。
- 输入集合固定、可重复，不依赖执行顺序和系统时间。
- 非恒等变换同时断言“确实发生变化”和“逆变换能恢复”，否则把变换改成恒等也会通过。

`tests/properties.rs` 当前覆盖：

- worker 计划不超过请求数和任务数，且内存缩减一定被披露。
- 手动镜头畸变与其逆变换在内部点往返。
- Lightroom mask warp 与其逆变换往返，并证明非恒等 warp 确实移动离中心点。
- recipe clamp 幂等。
- XMP 标量控件往返。
- 镜头 profile 的 distortion/CA 字段在组合后不会丢失。

## 故障注入的边界

故障注入只替代外部系统或不可控 I/O，不 mock 被测对象本身。测试必须断言产品结果，例如：

- 失败后旧文件仍然可读。
- 不留下半套 generation。
- 不能合并的 sidecar 被重新生成，并且损失通过 note 披露。
- 覆盖目标、只读目录和 symlink alias 在真正写入前被拒绝。

当前 `tests/store_behavior.rs` 中的 oversized commit、backup freeze 失败、clear backup 恢复和不可合并 XMP 用例属于这一层；`tests/fit_workflow.rs` 的缺失 segmentation sidecar 回退验证外部进程失败后的 recipe 和 claimed raster 生命周期。

## 定向 mutation 证据

以下为此前批次记录，不代表本轮重新执行的证据。2026-11-23 的独立复核、明确范围和执行结果见 `docs/HTTP_BEHAVIOR_AUDIT.md`。

此前记录的定向 mutation 发现：

- `jobs::plan_with_peak` 的 `<` 边界改成 `<=` 时，原测试没有发现“worker 数没有减少却错误披露”的情况；加入“note 当且仅当 worker 数被减少”后，定向 mutant 被杀死。
- `render::mask_warp_factor` 被改成恒等 `1.0` 时，原往返测试仍然通过；加入“非恒等 warp 必须移动离中心点”后，定向 mutant 被杀死。
- `render::geometry_profile` 删除 `distortion`、`ca_r`、`ca_b` 或 `ca_on` 字段时，原有测试没有全部发现；加入 profile 组合和 manual CA-only 用例后，5 个字段 mutant 全部被杀死。
- `fit_zoned::joint_bucket_err` 改成恒返 `0.0` 时，默认套件仍全部通过；加入“明显色彩差异必须产生非零 reading”后，定向 mutant 被杀死。
- `fit_zoned::semantic::resolve_regions` 的 `max_regions` 上限比较改成 `!=` 时，原测试仍通过；加入多区域截断用例后，定向 mutant 被杀死。
- `fit_zoned` 的 segmentation 失败回退：删除 claimed raster 的释放、替换 `ZONED_UNAVAILABLE` 披露、把 fallback recipe 重置为默认 recipe，三个错误反例分别被真实文件、披露和预览断言杀死；`segment_both` 恒返空 mask 的 `cargo-mutants` mutant 也被 `an_unavailable_segmentation_sidecar_delivers_a_global_fit_and_releases_its_claimed_mask` 捕获（`1 caught`）。
- `fit::fit_recipe_from` 的退化拒绝路径把 calibration base 改成默认 recipe 时，`a_degenerate_pair_keeps_the_calibration_base_in_the_refused_recipe` 在 `as_shot_k` 断言处变红；恢复后测试通过。
- `fit::fit_recipe_from` 的成功路径把初始 recipe 改成默认值、丢掉 base 时，`a_successful_fit_keeps_the_calibration_base_in_the_delivered_recipe` 在 `as_shot_k` 断言处变红；恢复后测试通过。
- `fit::evidence_model_paired` 把 target presence 过滤改成全 1 时，`evidence_model_with_no_present_target_pixels_has_no_population` 在 `population` 断言处变红；恢复后测试通过。
- `pipeline::preflight_out` 忽略 `guard_readonly` 结果时，已有照片和 symlink 两个输出目标用例同时变红；恢复后 product 行为套件通过。
- `segment::segment_file` 绕过缺失脚本检查时，`a_missing_segmentation_sidecar_is_reported_before_it_creates_a_mask` 在错误文案断言处变红；恢复后测试通过。
- `pipeline::write_xmp` 把 `durable_write` 换成原地 `std::fs::write` 时，`a_failed_xmp_publish_leaves_the_previous_projection_intact` 在错误返回断言处变红；恢复后测试通过。
- `pipeline::commit_analyzed_develop` 的 XMP 成员被跳过发布时，`an_analyzed_commit_after_its_commit_point_recovers_when_xmp_conflict_is_removed` 在首次冲突必须失败处变红；恢复后测试通过。
- `pipeline::produce_recipe` 绕过 harness key 前置检查时，`produce_recipe_without_a_harness_key_refuses_before_decoding` 在错误文案断言处变红；恢复后测试通过。
- `pipeline::migrate_recipe_coord_frame` 删除 baked 分支的 era stamp 时，`a_baked_image_with_legacy_coordinates_is_stamped_without_reframing` 在 era 断言处变红；恢复后测试通过。
- `decode::decode_any_turned` 删除 baked 分支的 Meta 尺寸更新时，`decoding_a_baked_image_with_a_quarter_turn_swaps_pixels_and_meta_dimensions` 在尺寸断言处变红；恢复后测试通过。
- `store::with_develop_lock` 让 `NoWait` 绕过锁时，`a_nowait_develop_lock_refuses_while_another_thread_holds_it` 在 `WouldBlock` 断言处变红；恢复后测试通过。
- 同一 mutation 下，跨进程 `a_develop_lock_serializes_across_processes` 也在 worker 持锁期间错误地拿到锁，`WouldBlock` 断言变红；恢复后测试通过。
- `decode::decode_raw_turned` 的无 preview 回退把 `quarter_turns` 改成 0 时，`a_raw_without_an_embedded_preview_honors_the_requested_quarter_turn` 在 `(16, 8) != (8, 16)` 处变红；恢复后测试通过。
- `segment::resolve_ai_masks` 绕过缺失脚本检查时，`resolving_an_ai_mask_with_a_missing_sidecar_reports_unresolved_and_keeps_no_raster` 在 unresolved 文案断言处变红；恢复后测试通过。
- `segment::resolve_ai_masks` 忽略可解码 cache hit 时，`resolving_an_ai_mask_reuses_a_decodable_cached_alpha_without_the_sidecar` 在 `cached == 1` 断言处变红；恢复后测试通过。
- `segment::segment_file` 忽略 `sidecar_wrote` 结果时，`a_segmentation_sidecar_that_exits_zero_without_output_is_a_failure` 在 exit-zero 无 artifact 文案处变红；恢复后测试通过。
- `render::stage_and_publish` 的失败分支不清理 staging 时，`a_failed_staged_export_publish_leaves_the_existing_delivery_untouched` 在目录数量断言处变红；恢复后测试通过。
- `face::skin_seed` 在空 mask 上返回伪造 seed 时，`skin_seed_returns_none_without_face_coverage` 在 `is_none` 断言处变红；恢复后测试通过。
- `render::convert_export_color_space` 把 Display P3 映射改成恒等时，`a_saturated_srgb_colour_changes_when_encoded_as_display_p3` 在通道移动断言处变红；恢复后测试通过。
- `render::as_shot_wb` 固定返回 5500K/0 时，`as_shot_wb_reads_finite_temperature_and_tint_from_dng_metadata` 在默认 anchor 断言处变红；恢复后测试通过。
- `xmp::merge_recipe_into_xmp` 每次合并追加额外标记时，`merging_the_same_recipe_twice_is_byte_stable` 在第二次输出比较处变红；恢复后测试通过。
- `render::camera_base_knots` 把非恒等结果强制成空 knots 时，`camera_base_knots_fit_a_known_tone_curve` 在 knots 数量断言处变红；恢复后测试通过。
- `face::resolve_face_masks` 在坏 report 上保留 face intent 时，`an_unusable_face_report_removes_face_intents_and_reports_the_reason` 在 masks 清空断言处变红；恢复后测试通过。
- `denoise::denoise_mosaic` 把零强度判断从 `<= 0` 改成 `< 0` 时，`a_zero_strength_mosaic_denoise_is_the_identity_without_a_sidecar` 在缺 sidecar 错误处变红；恢复后测试通过。
- `KillGroup::kill_tree` 把 `killpg` 改成只 kill group leader 时，`killing_a_sidecar_group_reaps_its_grandchild` 在 grandchild 存活断言处变红；恢复后测试通过。
- `produce_recipe` 把 deferred harness 返回的 recipe 换成默认 recipe 时，`a_fresh_analysis_returns_the_deferred_harness_recipe_and_verdict` 在 exposure/rationale 断言处变红；恢复后测试通过。
- `migrate_recipe_geometry` 把 era-0 的转向迁移强制成 `None` 时，`an_old_raw_recipe_migrates_its_crop_once_for_the_exif_turn` 在 `reframe` 缺失处变红；恢复后测试通过。
- `parse_dabs` 返回前清空 token stream 时，`an_external_brush_group_preserves_its_strokes_and_dab_stream` 在 dabs 断言处变红；恢复后测试通过。
- `parse_ai_mask` 返回前清空 provenance 时，`an_external_ai_mask_preserves_its_reference_provenance_and_gesture` 在 provenance 断言处变红；恢复后测试通过。
- `tag_icc` 强制写 sRGB profile 时，`a_delivered_tiff_carries_the_selected_icc_profile` 在两个 profile 必须不同的断言处变红；恢复后测试通过。
- `perspective::manual` 把两个 keystone 系数都清零时，水平与垂直两个 `frame_and_finish` case 各自在像素变化断言处变红；恢复后测试通过。
- `hdr::SdrRendition::global` 把 headroom 清零时，`an_hdr_rendition_compresses_a_highlight_without_lifting_black` 在亮部被压低断言处变红；恢复后测试通过。
- `denoise_mosaic` 丢弃 sidecar 产物并写入全零时，`a_successful_raw_denoise_adopts_the_same_sized_16_bit_mosaic` 在首像素断言处变红；恢复后测试通过。
- `run_sidecar_child` 不分配 kill group 时，`a_timed_out_denoise_sidecar_leaves_the_buffer_untouched_and_reaps_its_group` 在 grandchild 存活断言处变红；恢复后测试通过。
- `fit_recipe_from` 在返回前强制把 exposure 改成 +5 EV 时，`a_fit_from_a_calibration_base_does_not_worsen_the_delivered_preview` 在真实 preview 误差断言处变红；恢复后测试通过。
- `should_renew_cached_alpha` 强制返回 false 时，`a_fallback_alpha_is_renewed_when_the_primary_segmenter_becomes_available` 在 `renewed` 计数和 alpha 字节变化断言处变红；恢复后测试通过。
- `library::build` 忽略 previous library 时，`a_library_build_reuses_unchanged_embeddings_and_embeds_only_new_photos` 在第二次 build 的 embedding 调用计数处变红；恢复后测试通过。
- `parse_mask_brush_payload` 去掉 token 累加时，`an_aggregate_brush_table_imports_its_compressed_strokes` 在 brush stroke 导入断言处变红；恢复后测试通过。
- `serve::api_export` 在发布前截断临时文件时，`a_http_export_publishes_one_complete_registered_file` 在 JPEG 完整性断言处变红；恢复后测试通过。
- `serve::api_download` 在流式返回前截断临时文件时，`a_http_download_streams_one_complete_image_without_a_persistent_delivery` 在响应 JPEG 完整性断言处变红；恢复后测试通过。
- MCP `tools/list` 返回空列表时，`an_mcp_client_can_list_tools_and_read_a_photo_under_its_root` 在工具发现断言处变红；恢复后测试通过。
- MCP `Server::resolve` 接受根目录外 canonical 文件时，`an_mcp_frame_read_refuses_a_path_outside_its_root` 在 `isError` 断言处变红；恢复后测试通过。
- `library::best_settings` 忽略 `MATCH_DE_MAX` 时，`a_reference_sidecar_is_attached_only_when_it_renders_to_the_finished_photo` 在高 ΔE 成片仍附加 settings 的断言处变红；恢复后测试通过。
- `judge_pair` 把成功 score 改成 0 时，`a_successful_judge_result_is_parsed_through_the_pi_harness_boundary` 在 score 断言处变红；把失败文本附加 `DEEPSEEK_API_KEY` 时，`a_judge_failure_is_typed_and_does_not_leak_the_analysis_key` 在密钥泄漏断言处变红；恢复后测试通过。
- 测试政策 gate 加入自写 `SKIP` 行反例：临时加入 `println!("SKIP: synthetic policy probe")` 时，`check_test_policy.py` 在 `self-written skip line` 处失败；恢复后通过。
- `generative::reimagine` 的成功路径返回空字节时，`a_successful_reimagine_publishes_one_complete_generated_image` 在 PNG 完整性断言处变红；失败路径报错前写 partial 时，`a_failed_reimagine_does_not_install_a_partial_generated_image` 在 `!out.exists()` 断言处变红；恢复后测试通过。
- `global_render_gaps` 对所有控件报缺口时，`a_fully_rendered_global_recipe_has_no_render_gap_disclosures` 在空列表断言处变红；恢复后测试通过。
- `fit_zoned::segment_both` 强制失败时，`a_successful_zoned_fit_attaches_a_sky_mask_to_the_delivered_recipe` 在交付 recipe 缺少 raster-backed zone 的断言处变红；恢复后测试通过。
- `persist_postponed` 让 `WouldBlock` 走普通失败分支时，`a_busy_develop_is_reported_differently_from_a_real_io_failure` 在 busy 状态文案断言处变红；恢复后测试通过。
- `stamp_calibration` 无条件覆盖 `as_shot_k` 时，`stamping_calibration_preserves_saved_user_values_and_fills_missing_ones` 在保存值保留断言处变红；恢复后测试通过。
- `read_saved_develop` 跳过 recipe 恢复分支时，`opening_a_saved_recipe_restores_the_develop_without_a_loss_note` 在恢复断言处变红；恢复后测试通过。
- `read_saved_develop` 静默吞掉损坏 `recipe.json` 的解析错误时，`opening_a_corrupt_recipe_falls_back_to_xmp_and_reports_the_primary_loss` 在 `Unreadable` 断言处变红；恢复后测试通过。
- `read_saved_develop` 把中性 `recipe.json` 当成可恢复内容时，`a_neutral_recipe_does_not_resurrect_an_older_xmp_develop` 在 `NoopOnly` 断言处变红；恢复后测试通过。
- `read_saved_develop` 把不可读的 Lightroom sidecar 折成 absent 时，`an_unreadable_lightroom_sidecar_is_reported_while_the_store_recipe_still_opens` 在损失披露断言处变红；恢复后测试通过。
- `strip_from_record` 恢复背景卡时丢掉名称时，`restoring_a_variant_record_preserves_background_cards_and_their_identity` 在名称断言处变红；恢复后测试通过。
- 变体匹配忽略 `active_pos`、直接使用 `others` 下标时，`a_saved_variant_record_keeps_the_active_card_position_when_matching_live_cards` 在活动卡之后的位置匹配处变红；恢复后测试通过。
- `strip_from_record` 只 clamp recipe、不累计 `ClampSummary` 时，`restoring_an_oversized_variant_recipe_reports_the_cards_dropped_by_clamping` 在丢弃计数断言处变红；恢复后测试通过。
- `serve::api_develop` 把 web preview 的 `CropPolicy::Keep` 改成 `Cut` 时，`a_web_preview_keeps_the_full_frame_for_slider_feedback` 在返回 JPEG 尺寸从 16×12 变成 8×6 的断言处变红；恢复后测试通过。
- `fit_zoned::tile_evidence` 把 paired pixel 数强制为 0 时，`a_zoned_fit_discloses_supported_tiles_and_withholds_unsupported_range_bands` 在 `TILE_ELIGIBLE` 披露断言处变红；恢复后测试通过。该 case 仍只覆盖 range band 的拒绝披露，不冒充正面 band 生成已覆盖。
- `a_flat_zoned_pair_offers_no_spatial_tile_or_boundary_charge` 补充平坦 pair 的公开对照：没有 `TILE_ELIGIBLE`，也没有边界 charge 披露；与有纹理 case 一起覆盖 tile evidence 的接受/拒绝两侧。
- `fit_zoned::boundary_line_rims` 直接返回时，`a_feathered_semantic_region_boundary_is_measured_before_attachment` 在 `ZONE_BOUNDARY_PASSED(transitions>0)` 断言处变红；该 case 使用真实 multi-class feathered manifest 走 `TransitionBand`，恢复后测试通过。
- `fit_zoned::range::derive_colour_bands` 在返回前清空 proposals 时，`a_zoned_fit_attaches_a_measured_colour_range_when_one_hue_band_improves` 在缺少 `RangeMask::Color` 的断言处变红；恢复后测试通过。
- `fit_zoned::range::derive_luminance_bands` 在返回前清空 bands 时，`a_zoned_fit_attaches_a_measured_luminance_range_when_a_paired_look_improves` 在缺少 `RangeMask::Luminance` 的断言处变红；恢复后测试通过。
- `fit_zoned::spatial::eligible` 是私有 helper，正常公开工作流没有可区分的 parent-residual 输出；该项按“私有 helper 不单独立项”收口，公开 tile 选择由平坦、有纹理和真实 paired-look case 覆盖，不新增生产 seam。
- `fit_zoned::range` 关闭 luma evidence gate 时，`a_zoned_fit_reports_unsupported_luminance_and_colour_bands_without_attaching_them` 在 `RANGE_ABSTAINED` 的 1.5% population 原因断言处变红；恢复后测试通过。
- `fit_zoned::range` 关闭 hue evidence gate 时，同一 case 在 Orange/Aqua/Blue 的 `COLOUR_RANGE_ABSTAINED` 零结构证据原因断言处变红；恢复后测试通过。
- `quit::decide` 把 Dirty 的回复改成 `Now` 时，`a_dirty_quit_request_cancels_and_arms_the_in_app_close_prompt` 在 `Cancel` 断言处变红；恢复后测试通过。
- `PointColor::set_range` 改成只保存 slider 数值、不重建窗口时，`changing_a_point_colour_range_rebuilds_all_three_windows` 在 hue/sat/lum 窗口变化断言处变红；恢复后测试通过。
- `panel_heading` 把标题从 heading 改成 small 时，`the_panel_title_renders_larger_than_group_captions_with_a_short_rule` 在字号层级断言处变红；恢复后测试通过。
- `panel_heading` 把短规则线长度改成 0 时，同一 case 在 rendered line-segment 断言处变红；恢复后测试通过。
- crop rotated-corner reader 退化为 axis-aligned extents 时，`measured_lightroom_crops_produce_their_exported_dimensions` 在 P11 的交付尺寸从实测 8302×6277 漂到 8316.49×6258.26 处变红；恢复后测试通过。
- crop writer 把 `p*cos-q*sin` 的减号改成加号时，`measured_lightroom_crop_corners_survive_import_and_export` 在 P11 `CropLeft` 从 0.119389 漂到 0.120917 处变红；恢复后测试通过。
- crop overshoot 披露把 fraction 乘以源宽时，`crop_overshoot_is_trimmed_and_disclosed_in_straightened_axis_pixels` 在用户可见文案从 176 px 变成 264 px 处变红；恢复后测试通过。
- 公开 XMP reader 不接入 `RetouchAreas` 时，`lightroom_retouch_areas_stay_out_of_local_adjustment_counts` 在 retouch 从 1 个变成 0 个处变红；恢复后测试通过。
- 把 `heal_patchmatch + firefly` 误分为普通 content-aware 时，`lightroom_retouch_fill_modes_keep_their_known_origins` 在第三个 origin 错误处变红；恢复后测试通过。
- 接受只有 `SourceX` 的半个 donor 并补 y=0 时，`lightroom_retouch_donors_require_both_coordinates` 在残缺区域得到 `Some([0.6, 0.0])` 处变红；恢复后测试通过。
- ellipse 半径不从宽度单位换算为短边单位时，`lightroom_retouch_ellipse_becomes_a_scaled_heal_spot` 在 300×200 frame 的半径从 0.075 缩成 0.05 处变红；恢复后测试通过。
- 缺失 `crs:Feather` 时回退到 0 而不是 0.4 时，`lightroom_retouch_missing_feather_uses_a_safe_default` 在公开 heal plan 的硬边 feather 处变红；恢复后测试通过。
- 非空 brush 不进入 `retouch_spots` 时，`lightroom_retouch_brush_becomes_an_executable_heal_plan` 在 recipe 仍保留 stroke、但 heal plan 为空处变红；恢复后测试通过。
- HDR mode writer 写 `True` 而非 Lightroom 的 `1`，或 reader 把 absent/`True` 当开启时，`hdr_mode_uses_lightrooms_numeric_switch_and_strict_reader` 分别在公开 wire spelling 与严格 off 断言处变红；恢复后测试通过。
- HDR headroom writer 把 +1.50 降成 `+2` 时，`hdr_headroom_round_trips_as_a_signed_two_place_decimal` 在公开属性精度处变红；恢复后测试通过。
- HDR reader 忽略 `HDRMaxValue` 时，合成 headroom case 与 `lightrooms_hdr_sidecar_imports_its_mode_headroom_and_neutral_sdr_controls` 分别在 1.5 和真实 fixture 的 2.30 处变红；恢复后测试通过。
- SDR writer 跳过 `SDRContrast` 时，`hdr_sdr_rendition_controls_round_trip_as_signed_integers` 在缺失公开属性处变红；恢复后测试通过。
- amount carry 不再包含 `LuminanceNoiseReductionContrast` 时，`noise_reduction_amount_carries_its_zero_contrast_companion` 在 50 amount 下缺少 Contrast=0 处变红；恢复后测试通过。
- amount carry 错误包含非零默认的 `SharpenDetail` 时，`sharpening_amount_carries_only_its_zero_default_mask_companion` 在出现虚构 Detail=0 处变红；恢复后测试通过。
- XMP reader 跳过 sensor→display 坐标转向时，三条 `portrait_*` case 分别在 radial 中心、brush dabs 和 payload-free visible ellipse 处变红；恢复后测试通过。
- portrait brush writer 用 source aspect 代替 displayed aspect 时，`portrait_brush_imports_in_display_frame_and_roundtrips_sensor_dabs` 在原始 dab/radius 无法恢复处变红；恢复后测试通过。
- framed writer 把 `tiff:ImageWidth` 声明成 0 时，`portrait_framed_document_declares_and_roundtrips_visible_geometry` 在公开 source-frame 声明处变红；恢复后测试通过。
- orientation helper 忽略照片 EXIF、用 `(declared, declared)` 计算时，公开 composition、handedness refusal、matching-mirrors 三条 case 分别在 1→0 turn、缺失拒绝披露和 2→0 turns 处变红；恢复后测试通过。
- sidecar 未声明 orientation 却被强制增加一次 user turn 时，`orientation_contract_missing_declaration_keeps_zero_user_turn` 在公开 recipe 的 quarter_turns=1 处变红；恢复后测试通过。
- reader 只在宽高齐全时转 geometry 时，`orientation_contract_orientation_only_sidecar_places_and_roundtrips_mask` 在无矩形 sidecar 的 mask 仍停留于 sensor 坐标处变红；恢复后测试通过。
- recipe reader 移除 orientation-only 的 photo-frame fallback 时，`orientation_contract_photo_frame_restores_geometry_and_clears_false_losses` 在实际 pixel ellipse 不再等价处变红；恢复后测试通过。
- `import_losses_for_photo` 移除同一 fallback 时，该 case 在已恢复 geometry 仍被错误报告 Rotation loss 处变红；恢复后测试通过。
- `crop_import_note_for_photo` 移除同一 fallback 时，该 case 在已放置 crop 仍显示 “could not be placed” 假警告处变红；恢复后测试通过。
- orientation merge 的 disagreement arm 错误返回 Keep 时，`orientation_merge_updates_only_when_the_turn_changes` 在已移动照片仍写 Orientation=8 而非 1 处变红；恢复后测试通过。
- orientation merge 强制忽略 reader scope 时，`orientation_merge_discloses_an_unobservable_frame_change` 在把 Orientation 写进 reader 不采用的 Description 处变红；恢复后测试通过。
- reader-visible orientation correction 固定写 1 时，`orientation_merge_updates_reader_visible_frames_without_changing_visible_geometry` 在 Rotate90 应写 6 处变红；恢复后测试通过。删除 ancestor 下冗余 local `xmlns:tiff` 时该公开 case 仍绿，证明旧精确 tag 断言只锁实现布局，不迁移。
- brush correction 确定 base 后截断后续 components 时，`inline_brush_component_imports_beside_its_parametric_base_with_disclosure` 在 brush component 从 1 变 0 处变红；恢复后测试通过。
- Aggregate 从 base 候选移除时，`inline_brush_only_correction_uses_its_group_as_the_base` 在 brush-only correction 从 1 mask 变 0 处变红；恢复后测试通过。
- brush writer 不生成 Aggregate XML 时，`inline_brush_group_is_a_payload_free_crs_fixed_point_in_both_slots` 在公开写出路径失败；恢复后测试通过。
- brush parser 只收集已知 Paint、静默忽略未知 nested shape 时，`inline_brush_nested_paints_are_owned_once_and_unknown_nesting_is_refused` 在 smuggled Gradient 被错误接受处变红；恢复后测试通过。
- base selector 扫描整个 correction 时，`inline_brush_correction_ignores_shapes_outside_its_component_list` 选中 profile Look 的 Gradient，并在公开 import 发生 offset underflow；恢复后测试通过。
- base geometry 恢复为延伸到 correction 末尾的 unbounded slice 时，`inline_brush_component_attributes_cannot_invert_the_base_shape` 在后置 Aggregate 的 true bit 错误反转 linear base 处变红；恢复后测试通过。
- dab grammar 错误接受未知 `q` token 时，`inline_brush_unknown_encoding_is_refused_and_named` 在非法 Aggregate 被导入处变红；恢复后测试通过。
- `mask_coverage` 不再构建 brush runtime raster 时，`brush_mask_paints_its_dabs_without_covering_far_corners` 在首个 dab coverage 处变红；恢复后测试通过。
- brush runtime raster 被替换为 full-frame alpha 时，同一 case 在远角错误覆盖处变红；恢复后测试通过。
- brush dabs 使用普通字符 cap 而非 token-aware cap 时，`oversized_inline_brush_dabs_are_disclosed_and_republished_on_token_boundaries` 在 republished CRS 重读为 0 masks 处变红；恢复后测试通过。
- dab reader 移除 256-byte single-token bound 时，同一 case 在 300 KiB finite fractional token 被错误导入处变红；恢复后测试通过。
- range-mask writer 把 Intersect spelling 改成 Add 时，`range_masks_export_as_intersections_and_round_trip_publicly` 在两条 BlendMode/Inverted 文件断言处变红；恢复后测试通过。
- ProcessVersion 从 15.4 改成 15.0 时，`global_crs_projection_uses_lightrooms_public_spellings` 在公开 artifact 处变红；恢复后测试通过。
- tint-only writer 忽略已知 as-shot Kelvin 时，`tint_only_projection_uses_custom_at_the_stamped_kelvin` 在 Custom WB 变回 As Shot 处变红；恢复后测试通过。
- era-2 reader 忽略 `Autoshop 2` 时，`pre_rename_toolkit_names_keep_their_kelvin_era` 在错误 pin 5500 K 处变红；恢复后测试通过。
- merge 不升级 pre-rename `Autoshop` era-1 marker 时，`pre_rename_merge_upgrades_absolute_kelvin_to_the_current_era` 在旧 marker 留存处变红；恢复后测试通过。
- rationale scanner 不识别 Autoshop comment mark 时，`pre_rename_rationale_comment_is_refreshed_by_public_merge` 在 stale reason 留存处变红；恢复后测试通过。
- disclosure filter 静默忽略 NaN/inf 时，`non_finite_global_numbers_import_neutral_and_are_disclosed` 在中性化仍发生但诊断消失处变红；恢复后测试通过。
- out-of-domain curve points 被 clamp 而非整块拒绝时，`out_of_domain_global_curve_is_dropped_and_disclosed` 在错误 tone curve 被导入处变红；恢复后测试通过。
- wheel reader 无条件保留 saturation 时，`unreadable_color_grade_hue_clears_only_its_paired_saturation` 在 bogus hue 变成红色 0/50 wheel 处变红；恢复后测试通过。
- HSL writer 只输出非零 bands 时，`hsl_edits_export_the_complete_lightroom_band_block_and_round_trip` 在 24-key block 缩成 6 keys 处变红；恢复后测试通过。
- ColorGrade shadow hue 写到未验证 key 时，`color_grade_exports_lightrooms_split_toning_mapping_and_round_trips` 在缺失 SplitToningShadowHue 处变红；恢复后测试通过。
- merge 结果被错误替换为 fresh recipe document 时，`merge_preserves_foreign_wrappers_properties_and_children_across_remerge` 在 xpacket/foreign properties 丢失处变红；恢复后测试通过。
- owned property-element strip 被禁用时，`merge_replaces_owned_property_elements_once_and_preserves_unowned_elements` 在旧 Exposure element 留存处变红；恢复后测试通过。
- top-level strip 去掉 depth guard 时，`merge_strips_only_top_level_owned_elements_inside_a_creative_look` 在合法 nested Look merge 返回 None 处变红；恢复后测试通过。
- outer Description close 改成第一个 nested close 时，`merge_replaces_nested_mask_block_without_damaging_following_look` 在 nested mask merge 返回 None 处变红；恢复后测试通过。
- `save_master` 跳过 staged BakedGeometry stamp 时，`saved_masters_carry_replace_and_omit_baked_geometry_markers` 在 PNG marker 从 Some 变 None 处变红；恢复后测试通过。
- `SourceFrame::check` 无条件放行时，`unmarked_master_is_corrected_once_and_marked_master_is_refused` 在 marked frame 不再拒绝处变红；恢复后测试通过。
- `frame_and_finish` 禁用 lens geometry tail 时，同一 case 在 corrected rings 与 no-lens rings 变成相同处变红；恢复后测试通过。
- pixel-layer composite 忽略 recipe opacity 时，`pixel_layer_alpha_opacity_and_resampling_are_visible_in_public_preview` 在 0.5 blend 像素处变红；恢复后测试通过。
- `PixelLayer::draws` 忽略 enabled 时，`pixel_layer_switch_and_missing_file_have_preview_and_export_contracts` 在关闭图层仍覆盖 patch 处变红；恢复后测试通过。
- pixel-layer opacity clamp 被替换为固定 0.5 时，`pixel_layers_round_trip_in_recipe_json_and_clamp_opacity` 在 `[1,1,0]` 边界变成 `[0.5,0.5,0.5]` 处变红；恢复后测试通过。
- BasePart 错误保留完整 Look tone curve、取消 named-profile neutrality、或不显式关闭 base sharpening 时，`base_part_contains_only_the_named_profile_stage` 分别在公开 base recipe/neutral/capture-sharpening 断言处变红；恢复后测试通过。
- float thumbnail 退回 `image::thumbnail_exact` 时，`a_float_thumbnail_does_not_gain_half_a_code` 在多尺寸 fixture 出现约 0.500006 bias 处变红；恢复后测试通过。
- `stamp_mask_frame` 跳过 marker 时，`mask_raster_marker_names_its_frame_without_changing_pixels` 在 marked frame 从 Some 变 None 处变红；恢复后测试通过。
- `check_mask_frame` 无条件放行时，`mask_raster_mismatched_frame_is_refused_but_sensor_frame_is_portable` 在首个 mismatch 不再拒绝处变红；恢复后测试通过。
- corrected-raster sampling arm被禁用时，`corrected_mask_raster_covers_the_same_pixels_as_its_sensor_twin` 的matched误差升到1.42 codes并变红；恢复后测试通过。
- `into_sensor_gray` 跳过corrected→sensor inverse时，`corrected_mask_raster_reads_back_into_the_sensor_frame` 在sensor twin误差处变红；恢复后测试通过。
- XMP writer读取raster marker并注入mask block时，`sensor_and_corrected_mask_rasters_export_the_same_public_mask_xml` 在两个CRS block不相等处变红；恢复后测试通过。
- 同时绕过 `render_to_file_marked` 与 `render_baked_to_image` 的入口 clamp 时，极端但有限的蒙版几何把正常灰图渲染为黑色，`render_to_file_clamps_extreme_finite_mask_geometry_before_work` 变红；只绕过内层时外层仍保护交付结果，因此不冒充独立证据。
- `mask_raster_write_fits_budget` 忽略 recipe 中已有的 active raster 时，第二张 61 MP raster 被错误接受，`mask_raster_write_budget_counts_existing_active_rasters` 在 aggregate refusal 处变红；恢复后测试通过。
- mask snapshot loader 绕过 header-only aggregate peak guard 时，两个可单独解码的 61 MP rasters 被公开 render 一起接受并产生交付，`render_refuses_active_mask_rasters_over_the_aggregate_budget` 在错误返回处变红；恢复后测试通过。
- snapshot lookup 在像素应用前重新检查已加载 mask 的源文件是否存在时，diagnostics sink 同步删除文件后白 mask 变 inert，`preview_uses_a_loaded_mask_after_its_file_is_deleted` 在 preview 亮度断言处变红；恢复后测试通过。
- `refine_mask_guided` 不再把 NaN、负数和零 `eps` 提升到安全下限时，NaN 输出整张变黑，`guided_refine_floors_hostile_eps` 变红；恢复后测试通过。
- 空 guide 分支返回同尺寸黑图而不是输入蒙版时，`guided_refine_with_an_empty_guide_keeps_the_input_mask` 变红；恢复后测试通过。
- tiled guided refine 把 halo 清零时，恒定 guide 上的平滑线性 mask 在分块边界出现可见跳变或回退，`guided_refine_keeps_a_smooth_mask_free_of_visible_seams` 变红；恢复后测试通过。
- `source_pixels` 禁用 RAW dispatch branch 时，伪 `.arw` 被送进 finished-image decoder，`source_pixels_routes_raw_extensions_to_the_raw_decoder` 在公开错误来源处变红；恢复后测试通过。
- `source_pixels` 忽略 baked long-edge cap 时，`source_pixels_downscales_a_large_baked_file_to_the_requested_edge` 从 100×50 退回 400×200；对所有 `Some(edge)` 无条件 thumbnail 时，`source_pixels_does_not_enlarge_a_baked_file_below_the_requested_edge` 从 64×48 放大到 2048×1536。两处均已恢复。
- `render_baked_to_image` 忽略 working cap 或无条件 thumbnail 时，大图 cap 与小图 no-upscale case 分别红；恢复后通过。
- `render_baked_to_image` 把 working cap 从 develop 前移到最终 crop 后时，普通 cap 仍为 100×50，但 `baked_render_applies_the_working_cap_before_crop` 的半宽 crop 从 50×50 变成 100×100并变红；恢复后测试通过。
- opposing profile/manual vignette 恢复为两个各自 clamp 的 pass 时，公开亮部角落从 230 降到 188，`opposing_vignette_stages_cancel_before_highlights_are_clamped` 变红；恢复后测试通过。
- `EditRecipe::has_tone_work` 忽略 base curve 时，`a_base_curve_changes_pixels_without_user_tone_controls` 的 lift 消失；把 base/user tone 组合顺序从 `user(base(x))` 换成 `base(user(x))` 时，`user_exposure_composes_after_the_base_curve` 最大偏差 31 codes；把 base 黑端点抬到 0.1 时，`a_base_curve_keeps_black_pinned` 输出 26。三处均已恢复。
- global tone LUT、RGB curve pass 或 tone sliders 分别读取 creative Look 的 baked master curve、red curve 或 Shadows 时，`a_creative_looks_baked_tone_controls_do_not_reapply_to_pixels` 均在公开像素等价处变红；三处恢复后通过。
- `EditRecipe::renders_grayscale` 忽略 creative Look 的 monochrome switch 时，`a_monochrome_creative_look_delivers_greyscale_pixels` 从灰度退化为 `[200, 60, 40]` 并变红；恢复后测试通过。
- develop chain 跳过 `heal_planar` 时，ellipse 与 brush 两条公开 retouch 成片 case 都因黑物体仍可见而变红；恢复后测试通过。
- `retouch_spots` 把 width-unit radius 当作 short-side unit 或不从 absolute donor 减去 spot centre 时，已有 scaled-ellipse 与 donor 公共 case 分别红；恢复后通过。
- brush retouch plan 用默认值替代 area feather/origin，或清空 planner 生成的 exact coverage 时，`a_brush_retouch_plan_keeps_its_area_metadata_and_exact_shape` 分别红；恢复后测试通过。
- recipe orientation 跳过全部 retouch、只跳过 ellipse extent scale、brush rewrite 或 donor turn 时，`retouch_geometry_moves_with_a_quarter_turn` 分别红；让 inverse Rotate270 不使用交换后 frame 的 reciprocal radius scale 时，`retouch_geometry_returns_after_the_inverse_turn` 红。所有 mutation 均已恢复。
- `SdrRendition::global` 删除 HDR hard gate 或清零 SDR Brightness EV 时，`sdr_controls_are_inert_outside_hdr_edit_mode` 与 `sdr_brightness_lifts_midtones_inside_hdr_edit_mode` 分别红；恢复后通过。
- HDR shoulder 忽略 `sdr_blend` 时，`sdr_blend_scales_the_hdr_highlight_shoulder` 的 Blend −100 identity 断言红；把 stops clamp 到 1 EV 时，`hdr_headroom_keeps_growing_past_slider_limits` 的 1/2/4 EV 增长断言红；恢复后通过。
- HDR SDR-Clarity 使用比 Basic 大 5px 的 radius 时，`sdr_clarity_matches_basic_clarity_pixels` 在公开像素 parity 处红；恢复后测试通过。
- `camera_base_knots` 取消 identity empty verdict、把 tiny inability 折成 `Some(empty)`、或把黑端点改成 `[0.1,0.1]` 时，known/identity/tiny 三条公开 case 分别红；恢复后通过。
- camera base estimator 强制所有 interior knots 输出白点时，`a_sparse_camera_base_curve_keeps_the_delivered_ramp_rising_below_white` 在最长平台断言处红；恢复后测试通过。
- `block_lumas` 每个 8px block 只读第一个 camera pixel 时，`camera_base_knots_average_camera_texture_within_each_block` 在公开 knot 断言处红；恢复后通过。此前把 estimator group 代表从 midpoint 改成 lower bound 的尝试未被旧 fixture 杀死，证明 main 标题对“same-bin quantile mean”的解释不成立，因此未冒充 mutation 证据。
- `camera_frame_of` 无条件返回完整 neutral frame 时，`camera_base_look_pairs_a_center_crop_with_the_frame_the_camera_shows` 在 267×200 frame 断言处红；恢复后通过。
- corner selection 无条件选 lifted 时，unseen-lift 与 unjudgeable-fallback 两条公开 camera-base case 分别红；无条件选 plain 时 drew-lifted case 红。三处 mutation 均已恢复。
- profile lift 错误地在 camera crop 后应用时，公开 0.85 tone curve 最大误差从 `0.00119` 升到 `0.00585`，`camera_base_look_applies_the_lift_before_the_camera_crop` 在 `<0.003` 断言处红；恢复后通过。
- tone builder 在 recipe `names_profile()` 时跳过 base curve，`naming_a_camera_profile_does_not_replace_the_recipe_base_curve` 在公开像素 equality 处红；恢复后测试通过。
- `real_raw_base_look_moves_the_render_toward_the_camera_preview` 是显式真实 RAW gate；当前机器缺 `AUTOSHADE_PROBE_RAW`，显式运行按设计失败而非静默空测，因此尚无本机 mutation 证据，也不宣称该 gate 已绿。
- `as_shot_wb` 把 invalid zero-neutral result 伪造成 5500K/0 时，`as_shot_wb_returns_none_for_a_zero_camera_neutral` 红；恢复后测试通过。只删除早期 `<=0` guard 时 case 仍绿，因为下游 chromaticity 求解等价拒绝；该存活尝试不算 mutation 证据。
- lens blur 把 focus nearness 固定为 0.5 或忽略 enabled 开关时，focus 双向与 inert 配置 case 分别红；恢复后测试通过。
- lens blur 让每个 source pixel 以 full weight 进入 blur level 时，`focused_subject_does_not_bleed_into_the_blurred_background` 在相邻背景 halo 断言处红；恢复后通过。
- lens blur radius 固定为 512px frame 时，2x→1x mean error 从 `0.463` 升到 `4.537` codes，`lens_blur_scales_with_the_rendered_frame` 在 `<2.0` 处红；恢复后通过。
- depth loader 把 strict export 强制按 non-strict preview 处理时，missing depth 的 export 错误成功并返回 `(64,32)`，`develop_loads_depth_and_missing_depth_refuses_export_without_blurring_preview` 红；恢复后测试通过。
- LensBlur 移除 deny-unknown、把 amount ceiling 放宽到400、保留 empty protect 或 empty depth 时，JSON/clamp/path-cleanup 三条公开 case 分别红；恢复后通过。
- 48px black-protect fixture 下 radius×0.9/0.5 mutation 因 `<1px` level 量化存活，未算证据；放大到512px 后把 black 错读成50% protection，`a_black_lens_blur_protect_mask_matches_unprotected_delivery` 红。完全忽略 protect alpha 时 white/half sharpness case 红；均已恢复。
- protect loader 把 strict export 强制按 non-strict preview 处理时，missing protect 的 export 错误成功并返回 `(64,32)`，`develop_loads_lens_blur_protect_and_missing_protect_refuses_export` 红；恢复后通过。
- `engine_active` 删除 clarity/dehaze/texture/hue/sharpness family、把 local NR gate 改为“非零”、或删除四条 point-curve terms 时，三条 public activity case 分别红；恢复后通过。
- HSL chroma fade固定为1时，strong all-band controls让灰色移动64 codes，near-grey sky luma spread从受限状态升到84.1 codes；两条公开 case 均红并已恢复。
- public curve LUT 禁用缺省 `(0,0)/(1,1)` endpoint insertion 时，扩展后的 midpoint/inner/explicit-endpoint case 在black pin处红；恢复后通过。
- local mask pass 禁用 hue rotation 或把 signed sharpness 取绝对值时，公开 Hue 正/负方向与 Sharpness 强化/柔化 case 分别红；恢复后通过。
- mask tint 把 amount-zero adjustment 误判active时 activity断言红；只删除 `a*wgt` 因 zero-coverage early-return仍存而绿，不算证据；同时绕过两道 coverage gate 后 uncovered pixel变为 `[140,154,177]` 并红。所有修改已恢复。
- mask tint 把 lightness window固定为1、额外移动Oklab L、或把wheel旋转90°时，public pixels经标准Oklab读取的window/L/wheel断言分别红；恢复后通过。
- `real_mask_tint_matches_lightroom_and_camera_raw_exports` 是显式外部图片gate；当前机器无 tint corpus，不宣称其或对应mutation已绿。
- `camera_matrix` 对空矩阵回退 identity、或 `validate_calibration` 移除 determinant / degenerate-row / AsShotNeutral / 第四系数检查时，五条公开 RAW 校准拒绝 case 各自渲染成功 `(16,8)` 并红；均已恢复。
- `apply_lens_geometry` 让三个通道都用 red 的 LUT 时，`profile_ca_moves_each_channel_by_its_own_knots_only` 与 `ca_never_moves_green_even_when_a_channel_overshoots` 的 green byte-identical 断言红；`geometry_profile` 强制组合并给 fold 加 `1e-4` 偏移时，`a_rest_manual_ca_pair_renders_the_profile_byte_for_byte` 红。均已恢复。
- `apply_develop_with_rasters` 让 `passthrough` 进入 develop 时，`every_control_whose_tier_renders_nothing_moves_no_pixel` 报 passthrough 移动了像素；`Tier::PassThrough.renders()` 改为 true 时 premise `probed >= 1` 红。均已恢复。
- `lens_geom_norm` 把 CA 折进共享映射时，`the_shared_coordinate_map_round_trips_and_ignores_ca` 在 distortion 下 CA 开/关不等处红；已恢复。
- `develop_cfa_signed` 把黑电平减法 clip 到 0 时，`sub_black_noise_reaches_the_demosaic_instead_of_being_clamped` 的两次渲染完全相等（4207.4 vs 4207.4）并红；已恢复。首个 fixture 把 clamped 写成 +80 而非 +40，mutant 存活，已修正。
- `CoordFrame::brush_radius_scale` 固定为 1 时，`a_quarter_turn_rewrites_the_dab_stream_and_rescales_its_radii` 与 `a_turned_dab_is_still_a_circle_in_pixels` 红；`turned_dab_token` 交换坐标轴时，前者与 `a_turned_dab_lands_where_the_turned_radial_beside_it_does` 红；移除 Normal/Unknown early return 时 `an_unturned_photo_keeps_every_dab_byte` 红。均已恢复。
- `apply_masks` 不给 brush arm 传 raster 时，`the_develop_hands_the_brush_arm_its_raster` 的中心变暗断言红；已恢复。
- kernel cubic 系数翻转时 `brush_kernel_reproduces_the_measured_nine_rungs` 红；flow 律换成线性时 `brush_flow_law_matches_the_measured_deposit` 红；screen 换成 max 时 `brush_dabs_accumulate_by_screen_not_sum_or_max` 红。均已恢复。
- brush support 由 kernel early return、kernel `.max(0)`、循环 continue、行 chord 四层共同执行；单层或三层放宽的 mutant 均存活，四层一致延伸才让 `a_brush_dab_has_a_hard_support_edge_and_is_round_in_pixels` 在 texel 360 处红。已全部恢复。
- `mask_weight` 让 inverted 空组覆盖整帧时 `a_brush_group_with_no_drawable_dab_is_inert` 红；忽略组 inversion 时 `a_brush_groups_own_inversion_is_rendered` 红。均已恢复。

- `export.rs` 的 png 与 tif 编码器忽略 `eight_bit` 时 `export_depth_follows_the_option_not_the_extension` 红；已恢复。
- `apply_masks` 与 `mask_coverage` 的 dead-raster 守卫关闭时 `an_unresolved_ai_mask_skips_its_adjustment_instead_of_covering_the_frame` 在 `inverted=true` 处红。只把 `mask_weight` 的 `None => 0.0` 改成 `1.0` 时 case 仍绿，守卫先一步拦截；已恢复。
- `geometry_raster_path` 对 AiMask 返回 `None` 时 `a_zone_ai_mask_renders_exactly_like_the_bitmap_it_replaced` 红；已恢复。
- `LocalAdjustment::net_inverted` 忽略几何自带的位时 `ai_mask_import_preserves_authored_inversion_and_native_edits_win` 在 net 断言处红；渲染器 AI arm 不应用自身 inversion 时同一 case 在 coverage 断言处红。均已恢复。
- `orient_recipe_coords` 旋转时保留 AI 蒙版的旧 alpha 时 `turning_the_frame_moves_the_ai_click_and_invalidates_its_cached_alpha` 红；一律丢弃 alpha 时 `turning_the_frame_keeps_a_zone_masks_own_alpha_and_turns_it` 红。均已恢复。
- `cfa_taps` 的平面拟合退化为均值时 `a_linear_gradient_on_x_trans_renders_without_tile_period_chroma` 在 121 码处红（阈值 60，基线 12）；`wrap_table` 边界折叠改为镜像时 `an_x_trans_edge_does_not_ring` 与 flat 红；按帧宽折叠时 `a_flat_x_trans_mosaic_renders_flat_at_every_cfa_phase` 在 10073 码处红；R/B tap 集对调与关闭 `cfa_needs_geometry_demosaic` 分发时三条全红；四色分支不可达时 `a_four_colour_sensor_is_refused_by_name_and_renders_nothing` 红。均已恢复。
- 已知存活 mutant：去掉 `demosaic_over_cfa_geometry` 里的 `cfa.shift(roi.x(), roi.y())`。合成 DNG 无法表达带偏移 ActiveArea 的真实 X-S10 状态，ROI 原点相位这条行为目前没有公开执行者，需要真实 X-S10 RAF 的显式 gate。

这些结果比“覆盖率上涨”更能说明保护强度：测试不仅执行了代码，还证明了改坏产品行为会变红。

### 2026-11-23 当前复核新增证据

- 公开 `decode_raw` 对声明为 50001×1 CFA sensor plane 的最小 TIFF 返回明确的
  decoder-plane 上限错误。将 `guard_raw_plane_extent` 整体替换为 `Ok(())` 后，
  指定 case 在错误退化为“未知 RAW”处失败。最终以单行 diff 只选择该一个 mutant，
  基线通过并得到 `1 caught`。
- Harness Analyze 成功 case 分别捕获 `accept` 判定被删除和 revision 恒为 `None`；
  提交前失败 case 捕获子进程退出状态判断反转。三次均先通过指定基线，再由唯一目标
  case 失败；recipe / `saved` / 部分提交原子性仍未获得独立反例，不列为完成。
- 真实 CLI 子进程的 decode / apply 三条本地文件契约分别捕获
  `decode_cmd -> Ok(())`、`apply_cmd -> Ok(())` 和
  `guard_readonly -> Ok(())`。每次 diff 只选择一个 mutant，指定 case 得到
  `1 caught`；这不外推为其它 CLI 命令已验收。

## 运行方式

快速行为套件：

```bash
cargo test --locked --features gui
```

只跑性质测试：

```bash
cargo test --locked --features gui --test properties
```

定向 mutation：

```bash
PATH=/private/tmp/cargo-mutants/bin:$PATH cargo mutants \
  --no-config --cargo-arg=--locked --features gui \
  --cargo-test-arg=--all-targets \
  --file src/jobs.rs --re 'plan_with_peak' \
  --jobs 1 --timeout 300 --no-times --colors never
```

完整编码门禁：

```bash
scripts/agent_quality_gate.sh
```

该门禁先运行 `scripts/check_test_policy.py --self-test`，再扫描配置范围内的测试文件，拒绝已识别的源码嵌入、源码字符串、`assert!(true)`、自写 `SKIP` 行、裸 sleep、测试支持模块之外的环境写入和固定共享临时目录模式。通过仅说明这些模式未被检出，不能证明 premise 有效、断言不锁实现或所有写法均被识别。

架构约束只在 rustc、类型、可见性或 Clippy 有真实执行者时记为已验收。GUI
`logic/` 的 headless 边界由 `clippy.toml` 的 disallowed types/methods 和 module-scoped
deny 执行。此前针对 RAW 调用顺序、Store 读取、slot 生命周期、函数入口和文件布局的
源码正则扫描已删除：它们不能覆盖等价 Rust 写法，也不能证明资源拒绝、刷新或并发等
产品结果。相关 main cases 已重新打开，等待公开行为测试或不可绕过的类型设计。

此前的 `baked-load-image-reason` 只要求 `load_image` 前出现一条
`baked-by-construction` 注释。注释可以错误，合理重排也会改变扫描结果，因此它既
不能证明输入不是 RAW，也不是稳定的架构约束，已从自动 gate 删除。RAW / baked
分流若要成为机械门禁，需要由类型或不可绕过的入口表达；在此之前只作为评审项。

慢速、付费、真实 RAW、模型和网络路径仍必须保留显式 gate；gate 只决定运行时机，不决定是否纳入保护范围。

fit 的确定性慢测试使用：

```bash
scripts/fit_battery.sh
```

它会执行退化 pair、`fit_recipe_from` 的 calibration base 保留、完整 fit 行为链、`fit_zoned` 缺失 sidecar 回退和实际预览结果断言。Agent 门禁可用
`AUTOSHADE_FIT_TESTS=1 scripts/agent_quality_gate.sh` 显式启用。

测试专用慢用例仍要进入 mutation 验证。`mutation_gate.py` 在只有测试文件改动时不会静默跳过；必须通过 `AUTOSHADE_MUTATION_FILES=src/...` 指定被保护的生产文件。慢用例可同时传 `--test-arg=-- --test-arg=--ignored` 和测试名，让 mutation 真正运行对应 case。改动行没有 mutant 时不再退化为整份生产文件的无关候选：测试同时变更则要求显式行为范围，纯生产改动则失败并要求另给定向故障反例。

HTTP 的真实请求体边界与失败后容量恢复使用 `scripts/http_resource_battery.sh`，
由 build CI 显式执行；本地 Agent 用 `AUTOSHADE_HTTP_RESOURCE_TESTS=1` 启用。
本地小图像工作流仍在默认套件，资源测试的基线和定向反例记录见
`docs/MASTER_WORKFLOW_AUDIT.md`。

## 禁止做法

- 不以覆盖率作为个人、团队或 Agent 的考核、排名、OKR、奖金、CI 门槛或 PR 准入条件。
- 不按未覆盖行号批量补无行为语义的 case。
- 不用 `assert!(true)`、只检查函数存在、源码字符串扫描或调用次数替代行为断言。
- 不因为一个 case 快就保留它，也不因为一个 case 慢就删除它。
- 不为了测试在生产代码里加入隐藏开关。
- 不把恒等变换、vacuous property 或永远为真的断言当作性质测试。
