//! The export landing workflow: classify the worker result and compose its
//! line. The host applies status/toast/done, repairs the canvas and remembers
//! the destination.

use crate::*;

/// What the export landing asks the host to do.
pub(crate) enum ExportLanding {
    Failed(anyhow::Error),
    Single {
        relooked: bool,
        line: String,
        /// The line carries a mask warning: it must use the error channel.
        error: bool,
    },
    Batch {
        ok: usize,
        dest: PathBuf,
        line: String,
        /// At least one target failed: it must not read as a clean success.
        error: bool,
    },
}

pub(crate) fn land(lang: Lang, res: anyhow::Result<ExportOutcome>) -> ExportLanding {
    match res {
        Err(e) => ExportLanding::Failed(e),
        Ok(ExportOutcome::Single { out, relooked, warn }) => {
            let words = logic::export_landing::single_words(&abs_display(&out), relooked, warn.as_deref());
            let mut line = crate::i18n::text(lang, &words.line);
            if let Some(relook) = words.relook {
                line = format!("{line} — {}", crate::i18n::text(lang, &relook));
            }
            let error = words.warn.is_some();
            if let Some(warn) = words.warn {
                line = format!("{line} · {}", crate::i18n::text(lang, &warn));
            }
            ExportLanding::Single { relooked, line, error }
        }
        Ok(ExportOutcome::Batch { ok, errs, renamed, relooked, warns, dest }) => {
            let words = logic::export_landing::batch_words(
                ok,
                &errs,
                &renamed,
                relooked,
                &warns,
                &abs_display(&dest),
            );
            let mut line = crate::i18n::text(lang, &words.base);
            for part in [words.renamed, words.relooked, words.develops].into_iter().flatten() {
                line.push_str(&crate::i18n::text(lang, &part));
            }
            ExportLanding::Batch { ok, dest, line, error: !errs.is_empty() }
        }
    }
}
