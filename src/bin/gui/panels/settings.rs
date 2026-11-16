//! Settings panel and its form load/save.

use crate::*;



impl AutoShadeApp {
    pub(crate) fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let mut do_save = false;
        let mut fetch: Option<ModelRole> = None;
        // `lang` is a Copy snapshot so `tr`/`trf` never borrow `self` — the
        // `let f = &mut self.ui.settings` block below holds a partial borrow of self.
        let lang = self.lang;
        ui.label(
            // "& Theme", not "& Reverse-fit": the reverse-fit switch this
            // sentence used to point at now lives in the AI panel (R22 #4), and
            // Theme is the other control here that applies without a save.
            egui::RichText::new(tr(
                lang,
                "Language & Theme apply immediately. The provider sections below persist via 「Save settings」 to autoshade.local.json in your per-user AutoShade folder (never in a repo) and apply to the next AI call (Analyze / Fill / Generate image).",
            ))
            .weak()
            .small(),
        );
        ui.separator();
        ui.heading(tr(lang, "Language"));
        // English is the skeleton; Chinese is an overlay. Switching takes effect
        // next frame (every label re-reads `self.lang`), no restart/save needed.
        // `from_id_salt` is egui 0.29's name for the old `from_id_source`.
        egui::ComboBox::from_id_salt("lang_picker")
            .selected_text(self.lang.label())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.lang, Lang::En, Lang::En.label());
                ui.selectable_value(&mut self.lang, Lang::Zh, Lang::Zh.label());
            });
        ui.separator();
        ui.heading(tr(lang, "Theme"));
        // Four surface schemes and an accent picked apart from them (see
        // ThemeColors) — switching either re-installs the egui style this
        // frame and persists with the other prefs.
        let before = (self.ui.theme, self.ui.accent);
        egui::ComboBox::from_id_salt("theme_picker")
            .selected_text(tr(lang, self.ui.theme.label()))
            .show_ui(ui, |ui| {
                for t in ThemePref::ALL {
                    ui.selectable_value(&mut self.ui.theme, t, tr(lang, t.label()));
                }
            });
        ui.horizontal(|ui| {
            ui.label(tr(lang, "Accent colour"));
            for a in AccentPref::ALL {
                if accent_swatch(ui, a.hue(), self.ui.accent == a).on_hover_text(tr(lang, a.label())).clicked() {
                    self.ui.accent = a;
                }
            }
        });
        if (self.ui.theme, self.ui.accent) != before {
            install_look(ui.ctx(), self.ui.theme, self.ui.accent);
        }
        // 「Zoned fit (sky)」 MOVED to the AI panel's reverse-fit sub-area (R22
        // #4): it is a setting for ONE button, and it sat two panels away from
        // that button — you changed it here and then went hunting for 「🎛
        // Reverse-fit」. The `zoned_fit` FIELD is untouched (same Prefs key,
        // same persistence, same `start_fit` read); only its UI row moved.
        ui.separator();
        // The develop store is otherwise invisible (hashed AppData folders) —
        // this is the one place that names it and rescues pre-store saves.
        ui.heading(tr(lang, "Develop store"));
        // THIS photo's folder, not just the root (R22-8): the root is one line
        // above a hash-named subdirectory the user then has to identify by
        // guessing, and "where is my XMP" is the question this row exists to
        // answer. Falls back to the root when no photo is open, which is all
        // there is to say then.
        let shown = match self.photo.src_path.as_deref() {
            Some(p) => autoshade::store::develop_dir(p),
            None => autoshade::store::store_root(),
        };
        ui.label(egui::RichText::new(abs_display(&shown)).small().weak()).on_hover_text(tr(
            lang,
            "Where saved develops live: recipes, Lightroom XMP, version snapshots and mask rasters — one folder per photo, keyed by its absolute path. Override the location with the AUTOSHADE_DATA_DIR environment variable.",
        ));
        // Enabled only when the folder EXISTS: an unsaved photo has no develop
        // directory yet, and a file manager pointed at a missing path silently
        // opens somewhere else entirely (Explorer lands in Documents). One stat
        // per frame, but only while this window is open — a cached answer would
        // be wrong for exactly the case that matters (the user saves, then
        // reaches for this button). `develop_dir` above costs nothing repeated:
        // its identity resolution is memoized for the process (store::
        // identity_of), and `store_root` was already read here every frame.
        let exists = shown.is_dir();
        if action(ui, exists, tr(lang, "🗂 Show in file manager"))
            .on_hover_text(if exists {
                tr(lang, "Open this folder in your file manager")
            } else {
                tr(lang, "Nothing saved for this photo yet — the folder appears with the first save")
            })
            .clicked()
            && let Err(e) = reveal_folder(&shown)
        {
            let t = trf(lang, "could not open the folder: {err}", &[("err", &e.to_string())]);
            self.status = t.clone();
            self.toast(ToastKind::Error, t);
        }
        // The pixel masters (2026-10-18): the AI denoise / stack / heal /
        // clone / fill results cards are built FROM. They left the delivery
        // folder so cleaning exports cannot break a card, which also hid
        // them — this row names the folder and opens it.
        let masters = autoshade::store::masters_dir();
        ui.label(
            egui::RichText::new(trf(lang, "Pixel masters: {path}", &[("path", &abs_display(&masters))]))
                .small()
                .weak(),
        )
        .on_hover_text(tr(
            lang,
            "The full-resolution images AI denoise, stacking, heal, clone, fill and AI edits produce — the cards built on them read these files. Deleting one breaks its card, so delete the card first. Exports are NOT here (see 「Delivery folder」 below).",
        ));
        let has_masters = masters.is_dir();
        if action(ui, has_masters, tr(lang, "🗂 Show masters folder"))
            .on_hover_text(if has_masters {
                tr(lang, "Open this folder in your file manager")
            } else {
                tr(lang, "No master yet — the folder appears with the first AI denoise, stack or retouch")
            })
            .clicked()
            && let Err(e) = reveal_folder(&masters)
        {
            let t = trf(lang, "could not open the folder: {err}", &[("err", &e.to_string())]);
            self.status = t.clone();
            self.toast(ToastKind::Error, t);
        }
        if action(ui, true, tr(lang, "Import develops from an old ./out folder…"))
            .on_hover_text(tr(
                lang,
                "Saves made before v0.13 lived in a ./out folder next to wherever the app was launched. If your old edits are missing, point this at that folder — its recipes / XMP / versions migrate into the develop store.",
            ))
            .clicked()
        {
            self.apply_settings(logic::settings::SettingsAction::ImportLegacyFolder);
        }
        // --- the DELIVERY ROOT (R24-5 M8) ------------------------------------
        // The counterpart to the develop store above: that folder holds the
        // recipes, this one holds the finished files. It used to be the
        // hardcoded cwd-relative `./out` in five places at once (the CLI, the
        // batch renderer, the web download route, the GUI's destination
        // setting, the style-prompt writer), which is why "where did my export
        // go" had no answer but "wherever you launched the app from".
        ui.separator();
        ui.heading(tr(lang, "Delivery folder"));
        let resolved = autoshade::config::delivery_root();
        let mut browse_delivery = false;
        {
            let f = &mut self.ui.settings;
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut f.out_dir)
                        .desired_width(FIELD_W_MAX.min(ui.available_width() - 90.0).max(80.0))
                        .hint_text(autoshade::config::DEFAULT_DELIVERY_ROOT),
                )
                .on_hover_text(tr(
                    lang,
                    "Where finished files land: exports and the extracted style prompt — for this window, the CLI, the web surface and batch renders alike. Blank = the default ./out beside the working directory. Saved develops and the pixel masters cards are built on are NOT here (see 「Develop store」 above), so cleaning this folder never breaks a card.",
                ));
                if action(ui, true, tr(lang, "Browse…"))
                    .on_hover_text(tr(lang, "Pick the delivery folder"))
                    .clicked()
                {
                    browse_delivery = true;
                }
            });
        }
        if browse_delivery {
            self.apply_settings(logic::settings::SettingsAction::PickDeliveryFolder);
        }
        // The RESOLVED root, absolute: a relative setting (the default
        // included) means nothing without saying which directory it is
        // relative to — the same reason the export summary spells its target
        // out in full. Reflects the SAVED value, so it only moves after
        // 「Save settings」.
        ui.label(egui::RichText::new(abs_display(&resolved)).small().weak());
        // --- the PYTHON INTERPRETER (M1-3) -----------------------------------
        // WRITABLE here, unlike the sidecar path below, and that is not the
        // same ruling softened. `AUTOSHADE_PYTHON` stays `Trust::Destination`
        // — no `.env`, no working-directory `autoshade.local.json` beside
        // someone's photos may supply it — and this field writes ONLY the
        // trusted per-user file, which is the authority the environment
        // variable already had. What changed is REACHABILITY: a
        // Finder-launched `.app` inherits launchd's environment, not a
        // shell's, so on macOS an env-only setting is one no user of the
        // shipped app can set at all.
        //
        // The button probes interpreter NAMES through `PATH`
        // (`config::PYTHON_CANDIDATES`) — never a fixed machine path, never a
        // file dialog — so a value the user did not type by hand is one the
        // OS itself resolved.
        ui.separator();
        // ComfyUI (S5.11): the local generative backend's paths. Blank fields
        // derive the defaults (CLI from PATH, workspace/models from the Comfy
        // Desktop registry, output under the app store) — the value shown is
        // the resolved one, and it is what the next save pins.
        ui.heading(tr(lang, "ComfyUI (local generation)"));
        {
            let f = &mut self.ui.settings;
            let width = |ui: &egui::Ui| FIELD_W_MAX.min(ui.available_width() - 140.0).max(80.0);
            for (label, field) in [
                ("ComfyUI CLI", &mut f.comfy_cli),
                ("ComfyUI folder", &mut f.comfy_workspace),
                ("Models folder", &mut f.comfy_base_dir),
                ("Reimagine workflow", &mut f.comfy_reimagine_workflow),
                ("Fill workflow", &mut f.comfy_fill_workflow),
            ] {
                ui.horizontal(|ui| {
                    ui.label(tr(lang, label));
                    ui.add(egui::TextEdit::singleline(field).desired_width(width(ui)));
                });
            }
            ui.label(egui::RichText::new(tr(lang, "Blank = default")).small().weak());
        }
        // One resolved config for this row and the sidecar row below; the
        // panel used to load it once per row.
        let cfg_now = autoshade::config::Config::load();
        ui.heading(tr(lang, "Python interpreter"));
        {
            let f = &mut self.ui.settings;
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut f.python_bin)
                        .desired_width(FIELD_W_MAX.min(ui.available_width() - 90.0).max(80.0))
                        .hint_text(autoshade::config::default_python_bin()),
                )
                .on_hover_text(tr(
                    lang,
                    "Which Python runs the AI sidecars (segmentation, denoise, style). Blank = the managed environment when it is prepared, otherwise the platform default. It can only be set here or by the AUTOSHADE_PYTHON environment variable — never by a file that arrives beside your photos.",
                ));
                if action(ui, true, tr(lang, "Detect"))
                    .on_hover_text(tr(
                        lang,
                        "Look in the standard install locations for a working Python 3",
                    ))
                    .clicked()
                {
                    // `probe_python_bin` RUNS each candidate (`--version`)
                    // before offering it: on a Mac without developer tools
                    // the system `python3` exists and does not work, so an
                    // existence check alone would fill this field with a stub
                    // whose only behaviour is an install prompt.
                    match autoshade::config::probe_python_bin() {
                        Some(bin) => {
                            f.status = trf(lang, "found {bin}", &[("bin", &bin)]);
                            f.python_bin = bin;
                        }
                        // A real answer, said as one: "we looked in the
                        // standard places and found nothing" is actionable,
                        // a silently unchanged field is a button that did
                        // nothing.
                        None => {
                            f.status = tr(
                                lang,
                                "no Python found in the standard install locations — type the full path above",
                            )
                            .into();
                        }
                    }
                }
            });
        }
        // What the app will ACTUALLY run. The field above holds the FILE's
        // spelling and an `AUTOSHADE_PYTHON` in the environment outranks it,
        // so a user whose environment already answers would otherwise be
        // looking at an empty box.
        ui.label(egui::RichText::new(cfg_now.sidecar_python()).small().weak());
        // The managed environment: an app-owned interpreter carrying the
        // sidecars' wheels. Prepared only on request — the CUDA set is a
        // multi-gigabyte download, so it is never fetched in the background.
        ui.horizontal(|ui| {
            ui.label(tr(lang, "Managed Python"));
            match autoshade::python_env::readiness() {
                autoshade::python_env::Readiness::Ready(p) => {
                    ui.label(egui::RichText::new(p.display().to_string()).small())
                        .on_hover_text(tr(
                            lang,
                            "The AI sidecars run in this environment unless you name an interpreter above",
                        ));
                }
                autoshade::python_env::Readiness::NotPrepared => {
                    ui.label(egui::RichText::new(tr(lang, "not prepared")).small().weak());
                }
            }
        });
        if action(ui, !self.busy, tr(lang, "Prepare Python environment"))
            .on_hover_text(tr(
                lang,
                "Runs uv to create a private environment and install python/requirements-*.txt into it — a few gigabytes, downloaded once",
            ))
            .clicked()
        {
            self.apply_settings(logic::settings::SettingsAction::InstallPython);
        }
        if let Some(line) = &self.ui.python_env_line {
            ui.label(egui::RichText::new(line).small().weak());
        }
        // What the sidecars will actually launch — the launch check's answer,
        // which is the only place a wrong interpreter becomes visible before a
        // sidecar fails.
        if let Some(check) = &self.ui.python_check {
            use autoshade::python_env::PythonState;
            let fact = match &check.state {
                PythonState::Managed(_) => None,
                PythonState::Interpreter => Some(trf(
                    lang,
                    "AI sidecars use the interpreter named above: {bin}",
                    &[("bin", &check.bin)],
                )),
                PythonState::UnpreparedDefault => Some(trf(
                    lang,
                    "AI sidecars currently use {bin} — the managed environment is not prepared",
                    &[("bin", &check.bin)],
                )),
                PythonState::Unusable => Some(trf(
                    lang,
                    "AI sidecars need Python 3 — {bin} is not one",
                    &[("bin", &check.bin)],
                )),
            };
            if let Some(fact) = fact {
                ui.label(egui::RichText::new(fact).small().weak());
            }
        }
        // --- the SEGMENTATION SIDECAR path (R25, closing R22-1) --------------
        // R22 left "a settings row for the sidecar path" to R23; R23 and R24
        // did not do it, and this is why. `AUTOSHADE_SEGMENT_SCRIPT` names a
        // file that goes straight into `Command::new`, so `config.rs`
        // registers it `env_only(..., Trust::Destination)`: neither a `.env`
        // nor an ambient `autoshade.local.json` beside someone's photos may
        // supply it. Adding a picker here would write it into the trusted
        // settings file and hand every later launch a program chosen in a
        // dialog — the opposite of that ruling. The interpreter row ABOVE is
        // not a counter-example to it: that one offers a fixed candidate list
        // rather than a dialog, and it exists because a Finder-launched app
        // has no environment to read it from. This path is different on the
        // fact that decides it — it already has a working default inside the
        // app's own tree, so nobody needs a picker to make the app run.
        //
        // So this is READ-ONLY on purpose, and it says why. The two facts a
        // user actually needs are which file is resolved and whether it is
        // there; the existing 「did not ship the python sidecar」 line (the one
        // the two 🤖 buttons already show) is the answer for the missing case,
        // so the missing arm is not a second wording of the same thing.
        ui.separator();
        ui.heading(tr(lang, "Segmentation helper"));
        let seg = cfg_now.segment_script;
        let seg_path = std::path::PathBuf::from(&seg);
        ui.label(egui::RichText::new(abs_display(&seg_path)).small().weak()).on_hover_text(tr(
            lang,
            "Set with the AUTOSHADE_SEGMENT_SCRIPT environment variable (not editable here, because the app runs this file)",
        ));
        // `segment_helper_available` is the SAME predicate the two AI-select
        // buttons gate on (one `exists()` per process), so this row and those
        // buttons can never disagree about whether the helper is there.
        if !segment_helper_available() {
            ui.label(
                egui::RichText::new(tr(lang,
                    "This feature needs AutoShade's Python helper, which this download does not include. Install the full package, or (advanced) set AUTOSHADE_SEGMENT_SCRIPT to the path of segment.py",
                ))
                .small()
                .weak(),
            );
        }
        {
            let f = &mut self.ui.settings;
            // Fetched ids belong to the endpoint recorded at fetch time; once
            // that role's URL stops matching (typed edit, provider auto-swap),
            // they describe a DIFFERENT server — self-invalidate so the
            // pickers fall back to grounded defaults, not a stale menu.
            if !f.image_models.is_empty() && !same_base(&f.image_base_url, &f.image_models.from_base)
            {
                f.image_models.clear();
            }
            if !f.analysis_models.is_empty()
                && !same_base(&f.analysis_base_url, &f.analysis_models.from_base)
            {
                f.analysis_models.clear();
            }
            ui.separator();
            ui.heading(tr(lang, "Analysis model — used by AI Analyze, AI adjust and AI review"));
            ui.horizontal(|ui| {
                ui.label(tr(lang, "Provider"));
                let r1 = ui.radio_value(&mut f.analysis_provider_api, false, tr(lang, "OAuth (Claude CLI)"));
                let r2 = ui.radio_value(&mut f.analysis_provider_api, true, tr(lang, "API (OpenAI-compatible)"));
                if r1.changed() || r2.changed() {
                    // Without this the OTHER provider's model id stays in the
                    // field and the picker presents it as the current choice
                    // (a claude alias sent to an OpenAI endpoint, or vice
                    // versa). Swap to this provider's default on a flip —
                    // but ONLY what provably belongs to the other provider:
                    // see `analysis_model_on_flip` for what a name can and
                    // cannot decide here.
                    if let Some(m) =
                        analysis_model_on_flip(&f.analysis_model, f.analysis_provider_api)
                    {
                        f.analysis_model = m.into();
                    }
                }
            });
            // DeepSeek is OpenAI-compatible: one click fills the endpoint and
            // the model id, and the harness's fresh AI Analyze reads the key
            // entered below (analysis_api_key, falling back from the image
            // role's key). Without this the panel read as Claude-vs-OpenAI
            // only, which is not what this user has.
            ui.horizontal(|ui| {
                let label = tr(lang, "Use DeepSeek");
                if action(ui, true, label)
                    .on_hover_text(tr(
                        lang,
                        "Sets BOTH roles (proposer and verifier) to DeepSeek: Base URL https://api.deepseek.com, model deepseek-flash, Chat API family (DeepSeek does not accept OpenAI's json_schema contract). Enter your DeepSeek key in either API Key field; Save fills the other role's key when it is empty.",
                    ))
                    .clicked()
                {
                    f.analysis_provider_api = true;
                    f.analysis_base_url = autoshade::config::DEEPSEEK_BASE.to_string();
                    f.analysis_model = autoshade::config::DEEPSEEK_MODEL.to_string();
                    f.analysis_models.clear();
                    f.image_provider_oauth = false;
                    f.image_base_url = autoshade::config::DEEPSEEK_BASE.to_string();
                    f.image_model = autoshade::config::DEEPSEEK_MODEL.to_string();
                    f.image_models.clear();
                    // DeepSeek refuses OpenAI's response_format.json_schema;
                    // the chat family is the plain chat-completions path the
                    // legacy proposer/verifier already implement.
                    f.image_api_family = autoshade::config::DEEPSEEK_FAMILY.to_string();
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr(lang, "Model"));
                // OAuth uses Claude CLI aliases; API uses the ids fetched from
                // the ANALYSIS endpoint — its own probe, not the image role's.
                // Borrowing the image list meant a separate analysis endpoint
                // offered nothing but two hardcoded ids.
                let opts = if f.analysis_provider_api {
                    model_opts(&f.analysis_models.chat, &["gpt-5.5", "gpt-4o"], &f.analysis_model)
                } else {
                    model_opts(&[], &CLAUDE_ALIASES, &f.analysis_model)
                };
                model_picker(ui, "set_analysis_model", &mut f.analysis_model, &opts, lang);
            });
            ui.horizontal(|ui| {
                ui.label(tr(lang, "Reasoning effort"));
                // The CLI and the API endpoints accept different tier names —
                // offer the right list, and keep the free-text field for
                // anything a given endpoint adds later.
                // …and the model's OWN list where the listing said (part D):
                // a `/models` that names the tiers a model takes is the
                // detection the fixed suggestions stand in for.
                let tiers: Vec<&str> = if f.analysis_provider_api {
                    f.analysis_models.tiers_for(&f.analysis_model, &EFFORT_TIERS_API)
                } else {
                    EFFORT_TIERS_CLI.to_vec()
                };
                effort_picker(ui, "set_analysis_effort", &mut f.analysis_effort, &tiers, lang);
                if f.analysis_provider_api && f.analysis_models.efforts.contains_key(f.analysis_model.trim()) {
                    tiers_note(ui, tiers.is_empty(), lang);
                }
            });
            if f.analysis_provider_api {
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Base URL"));
                    ui.text_edit_singleline(&mut f.analysis_base_url);
                });
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "API Key"));
                    let hint = if f.analysis_key_present { tr(lang, "key set — blank keeps it") } else { tr(lang, "no key set") };
                    if ui
                        .add(egui::TextEdit::singleline(&mut f.analysis_api_key).password(true).hint_text(hint))
                        .changed()
                    {
                        // Availability is CREDENTIAL-dependent as well as
                        // URL-dependent — see the image role's key field.
                        f.analysis_models.clear();
                    }
                });
                ui.horizontal(|ui| {
                    let label = if f.analysis_models.fetching {
                        tr(lang, "fetching…")
                    } else {
                        tr(lang, "🔄 Fetch models")
                    };
                    if action(ui, !f.analysis_models.fetching, label)
                        .on_hover_text(tr(
                            lang,
                            "List the models THIS endpoint serves (GET /models). The analysis role has its own endpoint and key, so it gets its own list.",
                        ))
                        .clicked()
                    {
                        fetch = Some(ModelRole::Analysis);
                    }
                    if !f.analysis_models.chat.is_empty() {
                        ui.label(
                            egui::RichText::new(trf(
                                lang,
                                "{chat} chat",
                                &[("chat", &f.analysis_models.chat.len().to_string())],
                            ))
                            .weak()
                            .small(),
                        );
                    }
                });
            }
            ui.separator();
            ui.heading(tr(lang, "Image model — looks at your photo, and generates or edits images"));
            ui.horizontal(|ui| {
                ui.label(tr(lang, "Provider"));
                ui.radio_value(&mut f.image_provider_oauth, false, tr(lang, "API (OpenAI-compatible)"));
                ui.radio_value(&mut f.image_provider_oauth, true, tr(lang, "Sign in with ChatGPT (via the local Codex bridge)"));
            });
            // Flipping into OAuth while the endpoint is still empty or the stock
            // OpenAI host means the field is wrong for a subscription bridge —
            // swap in the loopback bridge default so it works without retyping.
            // Idempotent: stops once the user sets any other (custom) value.
            if f.image_provider_oauth {
                let b = f.image_base_url.trim();
                if b.is_empty() || b.trim_end_matches('/') == OPENAI_DEFAULT_URL {
                    f.image_base_url = CODEX_BRIDGE_URL.to_string();
                }
            } else {
                // Mirror image of the swap above: flipping BACK to API mode
                // with the auto-installed loopback bridge URL still in the
                // field would send real-API calls at a local bridge that may
                // not even be running. Idempotent, stops at custom values.
                let b = f.image_base_url.trim();
                if b.is_empty() || b.trim_end_matches('/') == CODEX_BRIDGE_URL.trim_end_matches('/') {
                    f.image_base_url = OPENAI_DEFAULT_URL.to_string();
                }
            }
            ui.horizontal(|ui| {
                let label = if f.image_models.fetching { tr(lang, "fetching…") } else { tr(lang, "🔄 Fetch models") };
                let clicked = action(ui, !f.image_models.fetching, label)
                    .on_hover_text(tr(
                        lang,
                        "List the models this endpoint serves (GET /models) so you can pick instead of guess — and a live reachability check for the bridge/API. Uses the key/token typed below; a saved key is only used at the endpoint it was saved for.",
                    ))
                    .clicked();
                if clicked {
                    fetch = Some(ModelRole::Image);
                }
                if !f.image_models.is_empty() {
                    let cn = f.image_models.chat.len().to_string();
                    let im = f.image_models.image_gen.len().to_string();
                    ui.label(
                        egui::RichText::new(trf(lang, "{chat} chat · {image} image", &[("chat", &cn), ("image", &im)]))
                            .weak()
                            .small(),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label(if f.image_provider_oauth { tr(lang, "Bridge URL") } else { tr(lang, "Base URL") });
                ui.text_edit_singleline(&mut f.image_base_url);
            });
            ui.horizontal(|ui| {
                ui.label(tr(lang, "Vision model"));
                let opts = model_opts(&f.image_models.chat, &["gpt-5.5", "gpt-4o"], &f.image_model);
                model_picker(ui, "set_vision_model", &mut f.image_model, &opts, lang);
                // The one thing the listing can say that a wrong pick would
                // otherwise only say at the first paid call.
                if f.image_models.vision_of(&f.image_model) == Some(false) {
                    ui.label(
                        egui::RichText::new(tr(lang, "⚠ takes no images, per /models"))
                            .color(ui.visuals().warn_fg_color)
                            .small(),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr(lang, "Reasoning effort"));
                let tiers = f.image_models.tiers_for(&f.image_model, &EFFORT_TIERS_API);
                effort_picker(ui, "set_image_effort", &mut f.image_effort, &tiers, lang);
                if f.image_models.efforts.contains_key(f.image_model.trim()) {
                    tiers_note(ui, tiers.is_empty(), lang);
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr(lang, "API style"));
                api_family_picker(ui, &mut f.image_api_family, lang);
            });
            ui.horizontal(|ui| {
                ui.label(tr(lang, "Image-gen model"));
                // OAuth (subscription) exposes gpt-image-2 first; API keys often
                // still prefer gpt-image-1.5 for its input_fidelity lock.
                let fallbacks: &[&str] = if f.image_provider_oauth {
                    &["gpt-image-2", "gpt-image-1.5"]
                } else {
                    &["gpt-image-1.5", "gpt-image-2", "gpt-image-1", "gpt-image-1-mini", "chatgpt-image-latest"]
                };
                let opts = model_opts(&f.image_models.image_gen, fallbacks, &f.image_gen_model);
                model_picker(ui, "set_imagegen_model", &mut f.image_gen_model, &opts, lang);
            });
            ui.horizontal(|ui| {
                ui.label(if f.image_provider_oauth { tr(lang, "Bridge password") } else { tr(lang, "API Key") });
                let hint = if f.image_key_present {
                    tr(lang, "set — blank keeps it")
                } else if f.image_provider_oauth {
                    tr(lang, "the password set in the bridge's own config (api-keys) — it stays on this computer and is not an OpenAI key")
                } else {
                    tr(lang, "no key set")
                };
                if ui
                    .add(egui::TextEdit::singleline(&mut f.image_api_key).password(true).hint_text(hint))
                    .changed()
                {
                    // Model availability is CREDENTIAL-dependent, not just
                    // URL-dependent: a different key at the same endpoint can
                    // serve a different catalogue, and the URL-keyed
                    // self-invalidation above can't see that. Typing a new
                    // key drops the fetched lists (the pickers fall back to
                    // grounded defaults until the next fetch).
                    f.image_models.clear();
                }
            });
            let note = if f.image_provider_oauth {
                tr(lang, "Uses your ChatGPT subscription through the Codex bridge (CLIProxyAPI) running on this computer — no OpenAI key. To set it up: download CLIProxyAPI from its GitHub releases, start it, sign in with your ChatGPT account when it asks, and paste the api-keys value from its configuration file as the bridge password above. Start the bridge before an edit, or the call cannot connect. Generated images are capped at about 1.5 MP by the subscription; for full-resolution edits switch to API with a real key.")
            } else {
                tr(lang, "Tip: gpt-image-1.5 keeps the photo most faithful (input_fidelity); newer models like gpt-image-2 ignore that lock and edit more freely.")
            };
            ui.label(egui::RichText::new(note).weak().small());
            ui.separator();
            ui.horizontal(|ui| {
                if primary(ui, true, tr(lang, "Save settings")).clicked() {
                    do_save = true;
                }
                if !f.status.is_empty() {
                    ui.label(egui::RichText::new(&f.status).weak().small());
                }
            });
        }
        // The icon's fine print (round-13 easter egg).
        ui.add_space(SPACE_SM);
        ui.label(
            egui::RichText::new(tr(
                lang,
                "skymanbp's AS — the “As” stands for AutoShade, not an Adobe subscription. Rent paid to date: $0.00.",
            ))
            .weak()
            .small(),
        );
        if do_save {
            self.apply_settings(logic::settings::SettingsAction::SaveForm);
        }
        if let Some(role) = fetch {
            self.apply_settings(logic::settings::SettingsAction::FetchModels(match role {
                ModelRole::Analysis => logic::settings::ModelSide::Analysis,
                ModelRole::Image => logic::settings::ModelSide::Image,
            }));
        }
    }
}

impl AutoShadeApp {
}

/// The launch line a check with something wrong gets; `None` when the sidecars
/// have a Python 3 and nothing has to be said.
pub(crate) fn python_startup_words(
    lang: Lang,
    check: &autoshade::python_env::PythonCheck,
) -> Option<String> {
    use autoshade::python_env::PythonState;
    match &check.state {
        PythonState::Managed(_) | PythonState::Interpreter => None,
        PythonState::UnpreparedDefault => Some(trf(
            lang,
            "The managed Python environment is not prepared — the AI sidecars would use {bin}. Prepare it in Settings",
            &[("bin", &check.bin)],
        )),
        PythonState::Unusable => Some(trf(
            lang,
            "The AI sidecars need Python 3 — {bin} did not report one. Prepare the managed environment in Settings, or name another interpreter",
            &[("bin", &check.bin)],
        )),
    }
}
