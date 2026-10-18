import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";

import type { LearningAnalysis } from "../types";

export function LearningAnalysisView({
  analysis,
  showHeading = true,
}: {
  analysis: LearningAnalysis;
  showHeading?: boolean;
}) {
  const { t } = useTranslation();
  const facts = [
    [t("learning.card.reading"), analysis.reading],
    [t("learning.analysis.baseForm"), analysis.base_form],
    [t("learning.analysis.partOfSpeech"), analysis.part_of_speech],
    [t("learning.analysis.register"), analysis.register],
  ].filter((entry): entry is [string, string] => Boolean(entry[1]));
  const segments = analysis.segments.filter((segment) => segment.text || segment.role || segment.explanation);
  const grammarPoints = analysis.grammar_points.filter((point) => point.form || point.meaning || point.note);
  const examples = analysis.examples.filter((example) => example.source || example.translation);
  const uncertainties = analysis.uncertainties.filter(Boolean);

  return (
    <section className="learning-analysis" aria-label={t("learning.analysis.title")}>
      {showHeading && (
        <div className="learning-section-heading">
          <h3>{t("learning.analysis.title")}</h3>
        </div>
      )}
      {analysis.current_meaning && <div className="learning-analysis-summary learning-analysis-meaning">
        <h4>{t("learning.analysis.currentMeaning")}</h4>
        <p>{analysis.current_meaning}</p>
      </div>}
      {analysis.summary && <div className="learning-analysis-summary">
        <h4>{t("learning.analysis.summary")}</h4>
        <p>{analysis.summary}</p>
      </div>}
      {facts.length > 0 && (
        <dl className="learning-analysis-facts">
          {facts.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
        </dl>
      )}
      {segments.length > 0 && (
        <section className="learning-analysis-block">
          <h4>{t("learning.analysis.segments")}</h4>
          <ol className="learning-analysis-segments">
            {segments.map((segment, index) => <li key={index}>
              <div className="learning-analysis-term">
                {segment.text && <strong>{segment.text}</strong>}
                {segment.role && <span>{segment.role}</span>}
              </div>
              {segment.explanation && <p>{segment.explanation}</p>}
            </li>)}
          </ol>
        </section>
      )}
      {grammarPoints.length > 0 && (
        <section className="learning-analysis-block">
          <h4>{t("learning.analysis.grammarPoints")}</h4>
          <ul className="learning-analysis-entries">
            {grammarPoints.map((point, index) => <li key={index}>
              {point.form && <strong>{point.form}</strong>}
              {point.meaning && <p>{point.meaning}</p>}
              {point.note && <p className="learning-analysis-note">{point.note}</p>}
            </li>)}
          </ul>
        </section>
      )}
      {analysis.memory_tip && (
        <section className="learning-analysis-block learning-analysis-tip">
          <h4>{t("learning.analysis.memoryTip")}</h4>
          <p>{analysis.memory_tip}</p>
        </section>
      )}
      {examples.length > 0 && (
        <section className="learning-analysis-block">
          <h4>{t("learning.analysis.examples")}</h4>
          <ul className="learning-analysis-entries learning-analysis-examples">
            {examples.map((example, index) => <li key={index}>
              {example.source && <p>{example.source}</p>}
              {example.translation && <p className="learning-analysis-note">{example.translation}</p>}
            </li>)}
          </ul>
        </section>
      )}
      {uncertainties.length > 0 && (
        <section className="learning-analysis-block learning-analysis-caveats">
          <h4>{t("learning.analysis.uncertainties")}</h4>
          <ul>{uncertainties.map((value, index) => <li key={index}>{value}</li>)}</ul>
        </section>
      )}
      <details className="learning-disclosure">
        <summary>{t("learning.analysis.details")}<ChevronDown size={15} aria-hidden="true" /></summary>
        <div className="learning-disclosure-body">
          <p className="learning-analysis-meta">{analysis.provider} · {analysis.model}</p>
          <p className="learning-analysis-meta">{t("learning.analysis.confidence", { value: String(analysis.confidence) })}</p>
          <p className="learning-analysis-meta">{t("learning.analysis.metadata", { task: analysis.task_type, prompt: analysis.prompt_version })}</p>
        </div>
      </details>
    </section>
  );
}
