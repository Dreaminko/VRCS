import { invoke, isTauri } from "@tauri-apps/api/core";
import { ExternalLink } from "lucide-react";
import { useState, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";

import projectLinks from "../../about/project-links.json";
import { useContributors } from "../../about/useContributors";

const communityLinks = ["github", "discord", "qq", "vrchat"] as const;

export function AboutSettingsSection({ version }: { version?: string }) {
  const { t } = useTranslation();
  const { cache, loading, error } = useContributors();
  const [message, setMessage] = useState("");

  const openLink = (event: MouseEvent<HTMLAnchorElement>, target: string) => {
    if (!isTauri()) return;
    event.preventDefault();
    void invoke("open_project_link", { target }).catch(() => setMessage(t("settings.about.openFailed")));
  };

  return (
    <div className="settings-section settings-section-active about-section" id="settings-panel-about" role="tabpanel" aria-labelledby="settings-tab-about">
      <div className="about-brand">
        <img src="/logos/VRCS_Logo.svg" alt="VRCS" />
        {version && <span>{t("settings.about.version", { version })}</span>}
      </div>

      <section className="system-settings-group about-contributors" aria-labelledby="about-contributors-title">
        <div className="section-heading">
          <div><h3 id="about-contributors-title">{t("settings.about.contributors")}</h3></div>
        </div>
        <p className="about-description">{t("settings.about.thanks")}</p>
        {cache && cache.contributors.length > 0 && (
          <div className="about-avatars">
            {cache.contributors.map((user) => (
              <a key={user.id} className="about-avatar" href={`https://github.com/${user.login}`}
                target="_blank" rel="noopener noreferrer" data-tooltip={user.login} aria-label={user.login}
                onClick={(event) => openLink(event, `contributor:${user.login}`)}>
                <span aria-hidden="true">{user.login[0].toUpperCase()}</span>
                <img src={user.avatar_url} alt="" width={40} height={40} loading="lazy"
                  onError={(event) => { event.currentTarget.hidden = true; }} />
              </a>
            ))}
          </div>
        )}
        <div className="about-contributor-status" role="status">
          {loading && <span>{t("common.loading")}</span>}
          {!loading && error && <span>{t(`settings.about.${error}`)}{cache && ` ${t("settings.about.cached")}`}</span>}
          {!loading && !error && cache?.contributors.length === 0 && <span>{t("settings.about.empty")}</span>}
        </div>
      </section>

      <section className="system-settings-group" aria-labelledby="about-community-title">
        <div className="section-heading"><div><h3 id="about-community-title">{t("settings.about.community")}</h3></div></div>
        <div className="about-links">
          {communityLinks.map((id) => (
            <div className="about-link-row" key={id}>
              <a href={projectLinks[id]} target="_blank" rel="noopener noreferrer" onClick={(event) => openLink(event, id)}>
                <strong>{t(`settings.about.links.${id}`)}<ExternalLink size={12} /></strong>
                <span>{projectLinks[id].replace("https://", "")}</span>
              </a>
            </div>
          ))}
        </div>
        {message && <p className="about-feedback" role="status">{message}</p>}
      </section>
    </div>
  );
}
