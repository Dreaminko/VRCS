import { Wrench } from "lucide-react";

import type { DebugRow } from "../settings-types";

export function DebugSettingsSection({ rows }: { rows: DebugRow[] }) {
  const debugRows = rows;
  return (
        <div className="settings-section settings-section-active debug-section" id="settings-panel-debug" role="tabpanel" aria-labelledby="settings-tab-debug">
          <div className="section-heading">
            <div><Wrench size={18} /><h2>Debug</h2></div>
          </div>
          <div className="debug-list">
            {debugRows.map((row) => (
              <div className="debug-row" key={row.label}>
                <span>{row.label}</span>
                <strong>{row.value}</strong>
              </div>
            ))}
          </div>
        </div>
  );
}
