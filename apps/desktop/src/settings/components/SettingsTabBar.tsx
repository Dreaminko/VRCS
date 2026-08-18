import type { ReactNode } from "react";
import {
  AudioLines,
  GraduationCap,
  Layers3,
  KeyRound,
  Languages,
  Link,
  SlidersHorizontal,
  Volume2,
  Wrench,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsCategory } from "../settings-types";

export function SettingsTabBar({
  activeCategory,
  onChange,
}: {
  activeCategory: SettingsCategory;
  onChange: (category: SettingsCategory) => void;
}) {
  const { t } = useTranslation();
  const categories = [
    { id: "system", label: t("settings.categories.system"), icon: <SlidersHorizontal size={18} /> },
    { id: "audio", label: t("settings.categories.audio"), icon: <Volume2 size={18} /> },
    { id: "recognition", label: t("settings.categories.recognition"), icon: <AudioLines size={18} /> },
    { id: "translation", label: t("settings.categories.translation"), icon: <Languages size={18} /> },
    { id: "api", label: t("settings.categories.api"), icon: <KeyRound size={18} /> },
    { id: "learning", label: t("settings.categories.learning"), icon: <GraduationCap size={18} /> },
    { id: "connections", label: t("settings.categories.connections"), icon: <Link size={18} /> },
    { id: "vr_overlay", label: t("settings.categories.vrOverlay"), icon: <Layers3 size={18} /> },
    { id: "debug", label: "Debug", icon: <Wrench size={18} /> },
  ] satisfies Array<{ id: SettingsCategory; label: string; icon: ReactNode }>;

  return (
    <div className="settings-tabbar-wrap">
      <div className="settings-tabbar" role="tablist" aria-label={t("settings.categories.label")}>
        {categories.map((category) => {
          const active = activeCategory === category.id;
          return (
            <button
              key={category.id}
              id={`settings-tab-${category.id}`}
              className={active ? "active" : ""}
              type="button"
              role="tab"
              aria-selected={active}
              aria-controls={`settings-panel-${category.id}`}
              aria-label={category.label}
              onClick={() => onChange(category.id)}
            >
              <span className="settings-tab-icon" aria-hidden="true">{category.icon}</span>
              <span className="settings-tab-label">{category.label}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
