import { useTranslation } from "react-i18next";

import type { LearningAnalysis } from "../../types";

export function LearningAnalysisView({
  analysis,
  showHeading = true,
}: {
  analysis: LearningAnalysis;
  showHeading?: boolean;
}) {
  const { t } = useTranslation();
  const facts = [
    [t("learning.analysis.currentMeaning"), analysis.current_meaning],
    [t("learning.analysis.baseForm"), analysis.base_form],
    [t("learning.analysis.partOfSpeech"), analysis.part_of_speech],
    [t("learning.analysis.register"), analysis.register],
  ].filter((entry): entry is [string, string] => Boolean(entry[1]));

  return (
    <section className="learning-analysis" aria-label={t("learning.analysis.title")}>
      {showHeading && (
        <div className="learning-section-heading">
          <div>
            <h3>{t("learning.analysis.title")}</h3>
            <p>{analysis.provider} · {analysis.model}</p>
          </div>
          <span>{t("learning.analysis.confidence", { value: String(analysis.confidence) })}</span>
        </div>
      )}
      <div className="learning-analysis-summary">
        <span>{t("learning.analysis.summary")}</span>
        <p>{analysis.summary}</p>
      </div>
      {facts.length > 0 && (
        <dl className="learning-analysis-facts">
          {facts.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
        </dl>
      )}
      <AnalysisList
        title={t("learning.analysis.segments")}
        values={analysis.segments.map((segment) => (
          [segment.text, segment.role, segment.explanation].filter(Boolean).join(" — ")
        ))}
      />
      <AnalysisList
        title={t("learning.analysis.grammarPoints")}
        values={analysis.grammar_points.map((point) => (
          [point.form, point.meaning, point.note].filter(Boolean).join(" — ")
        ))}
      />
      <AnalysisList
        title={t("learning.analysis.uncertainties")}
        values={analysis.uncertainties}
      />
      {analysis.memory_tip && (
        <div className="learning-analysis-block">
          <h4>{t("learning.analysis.memoryTip")}</h4>
          <p>{analysis.memory_tip}</p>
        </div>
      )}
      <AnalysisList
        title={t("learning.analysis.examples")}
        values={analysis.examples.map((example) => (
          [example.source, example.translation].filter(Boolean).join(" — ")
        ))}
      />
      <p className="learning-analysis-meta">
        {t("learning.analysis.metadata", {
          task: analysis.task_type,
          prompt: analysis.prompt_version,
        })}
      </p>
    </section>
  );
}

function AnalysisList({ title, values }: { title: string; values: string[] }) {
  const visible = values.filter(Boolean);
  if (!visible.length) return null;
  return (
    <div className="learning-analysis-block">
      <h4>{title}</h4>
      <ul>{visible.map((value, index) => <li key={`${value}-${index}`}>{value}</li>)}</ul>
    </div>
  );
}
