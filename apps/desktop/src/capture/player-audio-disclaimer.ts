import type { TFunction } from "i18next";

export interface PlayerAudioDisclaimerSection {
  key: "principle" | "terms" | "anticheat" | "privacy" | "disclaimer";
  title: string;
  paragraphs: string[];
}

export interface PlayerAudioDisclaimer {
  title: string;
  intro: string;
  sections: PlayerAudioDisclaimerSection[];
  readingStatus: (remainingSeconds: number) => string;
  confirm: string;
}

const SECTION_KEYS = ["principle", "terms", "anticheat", "privacy", "disclaimer"] as const;

export function playerAudioDisclaimer(t: TFunction, os?: string): PlayerAudioDisclaimer {
  const prefix = os === "linux" ? "warnings.playerAudio.linux" : "warnings.playerAudio";
  const principleExtra = os === "linux" ? "warnings.playerAudio.linux.privileges" : "warnings.playerAudio.paging";
  return {
    title: t(`${prefix}.title`),
    intro: t(`${prefix}.intro`),
    sections: SECTION_KEYS.map((key) => ({
      key,
      title: t(`${prefix}.${key}Title`),
      paragraphs: [
        t(`${prefix}.${key}`),
        ...(key === "principle" ? [t(principleExtra), t("settings.audio.players.nameHint")] : []),
        ...(key === "privacy" ? [t(`${prefix}.privacyConsent`)] : []),
      ],
    })),
    readingStatus: (remainingSeconds: number) =>
      remainingSeconds > 0 ? t(`${prefix}.countdown`, { seconds: remainingSeconds }) : t(`${prefix}.ready`),
    confirm: t(`${prefix}.confirm`),
  };
}
