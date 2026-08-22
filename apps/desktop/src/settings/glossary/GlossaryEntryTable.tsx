import { ArrowDown, ArrowUp, FileUp, Plus, Search, Trash2 } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import type { GlossaryEntry } from "../types";
import { DropdownField, type DropdownOption } from "../../shared/ui/DropdownField";
import {
  applyGlossaryEntryBulkAction,
  visibleGlossaryEntries,
  type GlossaryEntryBulkAction,
  type GlossaryEntryCategoryFilter,
  type GlossaryEntryMappingFilter,
  type GlossaryEntrySortDirection,
  type GlossaryEntrySortKey,
} from "./glossary-entry-table";
import {
  GLOSSARY_CATEGORIES,
  MAX_GLOSSARY_ENTRIES,
  MAX_GLOSSARY_TERM_LENGTH,
  glossaryEntryDraft,
  glossaryEntryIssues,
  type GlossaryEntryDraft,
} from "./glossary-utils";

interface GlossaryEntryTableProps {
  entries: GlossaryEntryDraft[];
  fileInputRef: RefObject<HTMLInputElement | null>;
  saving: boolean;
  showValidation: boolean;
  onChange: (entries: GlossaryEntryDraft[]) => void;
}

export function GlossaryEntryTable({
  entries,
  fileInputRef,
  saving,
  showValidation,
  onChange,
}: GlossaryEntryTableProps) {
  const { t } = useTranslation();
  const tableScrollRef = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<GlossaryEntryCategoryFilter>("all");
  const [mapping, setMapping] = useState<GlossaryEntryMappingFilter>("all");
  const [sortKey, setSortKey] = useState<GlossaryEntrySortKey>("position");
  const [sortDirection, setSortDirection] = useState<GlossaryEntrySortDirection>("asc");
  const [selectedRowIds, setSelectedRowIds] = useState<Set<string>>(() => new Set());
  const [bulkAction, setBulkAction] = useState<GlossaryEntryBulkAction | "">("");

  const categoryOptions = useMemo<DropdownOption[]>(() => GLOSSARY_CATEGORIES.map((value) => ({
    value,
    label: t(`settings.glossary.glossaryCategories.${value}`),
  })), [t]);
  const categoryFilterOptions = useMemo<DropdownOption[]>(() => [
    { value: "all", label: t("settings.glossary.table.allCategories") },
    ...categoryOptions,
  ], [categoryOptions, t]);
  const mappingOptions = useMemo<DropdownOption[]>(() => [
    { value: "all", label: t("settings.glossary.table.allMappings") },
    { value: "translated", label: t("settings.glossary.table.fixedTranslations") },
    { value: "original", label: t("settings.glossary.keepOriginal") },
  ], [t]);
  const bulkActionOptions = useMemo<DropdownOption[]>(() => [
    { value: "", label: t("settings.glossary.table.chooseBulkAction") },
    ...categoryOptions.map((option) => ({
      value: `category:${option.value}`,
      label: t("settings.glossary.table.setCategory", { category: option.label }),
    })),
    { value: "target:keep", label: t("settings.glossary.table.setKeepOriginal") },
    { value: "target:translate", label: t("settings.glossary.table.setFixedTranslation") },
    { value: "case:sensitive", label: t("settings.glossary.table.setCaseSensitive") },
    { value: "case:insensitive", label: t("settings.glossary.table.setCaseInsensitive") },
  ], [categoryOptions, t]);

  const visibleRows = useMemo(() => visibleGlossaryEntries(
    entries,
    query,
    category,
    mapping,
    sortKey,
    sortDirection,
  ), [category, entries, mapping, query, sortDirection, sortKey]);
  const issues = useMemo(() => glossaryEntryIssues(entries, t), [entries, t]);
  const issueByCell = useMemo(() => new Map(issues.map((issue) => {
    const rowId = entries[issue.row - 1]?.rowId;
    return [`${rowId}:${issue.field}`, issue.message];
  })), [entries, issues]);
  const visibleRowIds = useMemo(() => visibleRows.map(({ entry }) => entry.rowId), [visibleRows]);
  const allVisibleSelected = visibleRowIds.length > 0
    && visibleRowIds.every((rowId) => selectedRowIds.has(rowId));
  const someVisibleSelected = visibleRowIds.some((rowId) => selectedRowIds.has(rowId));

  useEffect(() => {
    const validRowIds = new Set(entries.map((entry) => entry.rowId));
    setSelectedRowIds((current) => {
      const next = new Set([...current].filter((rowId) => validRowIds.has(rowId)));
      return next.size === current.size ? current : next;
    });
  }, [entries]);

  useEffect(() => {
    const firstIssue = issues[0];
    if (!showValidation || !firstIssue) return;
    const rowId = entries[firstIssue.row - 1]?.rowId;
    if (!rowId) return;
    setQuery("");
    setCategory("all");
    setMapping("all");
    setSortKey("position");
    window.requestAnimationFrame(() => window.requestAnimationFrame(() => {
      const cell = tableScrollRef.current?.querySelector<HTMLElement>(
        `[data-row-id="${rowId}"] [data-field="${firstIssue.field}"]`,
      );
      cell?.scrollIntoView({ block: "center", inline: "nearest" });
      cell?.focus({ preventScroll: true });
    }));
  }, [entries, issues, showValidation]);

  const updateEntry = (rowId: string, patch: Partial<GlossaryEntry>) => onChange(entries.map((entry) => (
    entry.rowId === rowId ? { ...entry, ...patch } : entry
  )));
  const removeRows = (rowIds: ReadonlySet<string>) => {
    if (!rowIds.size) return;
    if (!window.confirm(t("settings.glossary.table.confirmDeleteEntries", { count: rowIds.size }))) return;
    onChange(entries.filter((entry) => !rowIds.has(entry.rowId)));
    setSelectedRowIds(new Set());
  };
  const updateSort = (nextKey: GlossaryEntrySortKey) => {
    if (nextKey === "position") {
      setSortKey("position");
      setSortDirection("asc");
      return;
    }
    if (sortKey === nextKey) {
      setSortDirection((current) => current === "asc" ? "desc" : "asc");
      return;
    }
    setSortKey(nextKey);
    setSortDirection("asc");
  };

  return (
    <div className="glossary-entry-workspace">
      <div className="glossary-entry-toolbar">
        <div className="glossary-entry-search">
          <Search size={14} aria-hidden="true" />
          <input
            type="search"
            value={query}
            disabled={saving}
            aria-label={t("settings.glossary.table.search")}
            placeholder={t("settings.glossary.table.searchPlaceholder")}
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>
        <DropdownField
          compact
          floating
          floatingLayer="dialog"
          label={t("settings.glossary.glossaryCategory")}
          value={category}
          options={categoryFilterOptions}
          disabled={saving}
          onChange={(value) => setCategory(value as GlossaryEntryCategoryFilter)}
        />
        <DropdownField
          compact
          floating
          floatingLayer="dialog"
          label={t("settings.glossary.table.mappingFilter")}
          value={mapping}
          options={mappingOptions}
          disabled={saving}
          onChange={(value) => setMapping(value as GlossaryEntryMappingFilter)}
        />
        <div className="glossary-entry-toolbar-actions">
          <button
            className="secondary-button"
            type="button"
            disabled={saving}
            onClick={() => fileInputRef.current?.click()}
          >
            <FileUp size={14} aria-hidden="true" />
            {t("settings.glossary.importGlossaryJson")}
          </button>
          <button
            className="secondary-button"
            type="button"
            disabled={saving || entries.length >= MAX_GLOSSARY_ENTRIES}
            onClick={() => {
              setQuery("");
              setCategory("all");
              setMapping("all");
              setSortKey("position");
              onChange([...entries, glossaryEntryDraft()]);
            }}
          >
            <Plus size={14} aria-hidden="true" />
            {t("settings.glossary.addGlossaryEntry")}
          </button>
        </div>
      </div>

      <div className="glossary-entry-table-scroll" ref={tableScrollRef} data-floating-boundary>
        <table className="glossary-entry-table">
          <thead>
            <tr>
              <th className="glossary-entry-select-cell" scope="col">
                <SelectionCheckbox
                  checked={allVisibleSelected}
                  indeterminate={!allVisibleSelected && someVisibleSelected}
                  disabled={saving || visibleRows.length === 0}
                  label={t("settings.glossary.table.selectVisible")}
                  onChange={() => setSelectedRowIds((current) => {
                    const next = new Set(current);
                    if (allVisibleSelected) visibleRowIds.forEach((rowId) => next.delete(rowId));
                    else visibleRowIds.forEach((rowId) => next.add(rowId));
                    return next;
                  })}
                />
              </th>
              <SortableHeader
                className="glossary-entry-position-cell"
                label="#"
                sortKey="position"
                activeSortKey={sortKey}
                direction={sortDirection}
                onSort={updateSort}
              />
              <SortableHeader label={t("settings.glossary.glossarySource")} sortKey="source" activeSortKey={sortKey} direction={sortDirection} onSort={updateSort} />
              <SortableHeader label={t("settings.glossary.glossaryTarget")} sortKey="target" activeSortKey={sortKey} direction={sortDirection} onSort={updateSort} />
              <SortableHeader className="glossary-entry-category-cell" label={t("settings.glossary.glossaryCategory")} sortKey="category" activeSortKey={sortKey} direction={sortDirection} onSort={updateSort} />
              <th className="glossary-entry-toggle-cell" scope="col">{t("settings.glossary.keepOriginal")}</th>
              <th className="glossary-entry-toggle-cell" scope="col">{t("settings.glossary.caseSensitive")}</th>
              <th className="glossary-entry-action-cell" scope="col" aria-label={t("settings.glossary.table.actions")} />
            </tr>
          </thead>
          <tbody>
            {visibleRows.map(({ entry, index }) => (
              <GlossaryEntryRow
                key={entry.rowId}
                entry={entry}
                position={index + 1}
                selected={selectedRowIds.has(entry.rowId)}
                saving={saving}
                categoryOptions={categoryOptions}
                sourceIssue={showValidation ? issueByCell.get(`${entry.rowId}:source`) : undefined}
                targetIssue={showValidation ? issueByCell.get(`${entry.rowId}:target`) : undefined}
                onSelect={() => setSelectedRowIds((current) => {
                  const next = new Set(current);
                  if (next.has(entry.rowId)) next.delete(entry.rowId);
                  else next.add(entry.rowId);
                  return next;
                })}
                onUpdate={(patch) => updateEntry(entry.rowId, patch)}
                onRemove={() => {
                  onChange(entries.filter((item) => item.rowId !== entry.rowId));
                  setSelectedRowIds((current) => {
                    const next = new Set(current);
                    next.delete(entry.rowId);
                    return next;
                  });
                }}
              />
            ))}
          </tbody>
        </table>
        {visibleRows.length === 0 && (
          <p className="glossary-dialog-empty">
            {entries.length
              ? t("settings.glossary.table.noMatchingEntries")
              : t("settings.glossary.noGlossaryEntries")}
          </p>
        )}
      </div>

      <div className="glossary-entry-statusbar">
        <span>{t("settings.glossary.table.visibleCount", { visible: visibleRows.length, total: entries.length })}</span>
        {selectedRowIds.size > 0 ? (
          <div className="glossary-entry-bulk-actions">
            <strong>{t("settings.glossary.table.selectedCount", { count: selectedRowIds.size })}</strong>
            <DropdownField
              compact
              floating
              floatingLayer="dialog"
              floatingWidth={200}
              label={t("settings.glossary.table.bulkAction")}
              value={bulkAction}
              options={bulkActionOptions}
              disabled={saving}
              onChange={(value) => setBulkAction(value as GlossaryEntryBulkAction | "")}
            />
            <button
              className="secondary-button"
              type="button"
              disabled={saving || !bulkAction}
              onClick={() => {
                if (!bulkAction) return;
                onChange(applyGlossaryEntryBulkAction(entries, selectedRowIds, bulkAction));
              }}
            >
              {t("settings.glossary.table.applyBulkAction")}
            </button>
            <button
              className="api-row-icon-button api-danger-button"
              type="button"
              disabled={saving}
              aria-label={t("settings.glossary.table.deleteSelected")}
              title={t("settings.glossary.table.deleteSelected")}
              onClick={() => removeRows(selectedRowIds)}
            >
              <Trash2 size={15} aria-hidden="true" />
            </button>
          </div>
        ) : (
          <small>{t("settings.glossary.glossaryEntryLimit", { count: MAX_GLOSSARY_ENTRIES })}</small>
        )}
      </div>
    </div>
  );
}

const GlossaryEntryRow = memo(function GlossaryEntryRow({
  entry,
  position,
  selected,
  saving,
  categoryOptions,
  sourceIssue,
  targetIssue,
  onSelect,
  onUpdate,
  onRemove,
}: {
  entry: GlossaryEntryDraft;
  position: number;
  selected: boolean;
  saving: boolean;
  categoryOptions: DropdownOption[];
  sourceIssue?: string;
  targetIssue?: string;
  onSelect: () => void;
  onUpdate: (patch: Partial<GlossaryEntry>) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  return (
    <tr className={selected ? "selected" : ""} data-row-id={entry.rowId}>
      <td className="glossary-entry-select-cell">
        <SelectionCheckbox checked={selected} disabled={saving} label={t("settings.glossary.table.selectRow", { row: position })} onChange={onSelect} />
      </td>
      <th className="glossary-entry-position-cell" scope="row">{position}</th>
      <td className={sourceIssue ? "invalid" : undefined}>
        <input
          data-field="source"
          maxLength={MAX_GLOSSARY_TERM_LENGTH}
          value={entry.source}
          disabled={saving}
          aria-label={t("settings.glossary.table.sourceAtRow", { row: position })}
          aria-invalid={Boolean(sourceIssue)}
          title={sourceIssue}
          onChange={(event) => onUpdate({ source: event.target.value })}
        />
      </td>
      <td className={targetIssue ? "invalid" : undefined}>
        <input
          data-field="target"
          maxLength={MAX_GLOSSARY_TERM_LENGTH}
          value={entry.target ?? ""}
          disabled={saving || entry.target === null}
          aria-label={t("settings.glossary.table.targetAtRow", { row: position })}
          aria-invalid={Boolean(targetIssue)}
          placeholder={entry.target === null ? t("settings.glossary.keepOriginal") : undefined}
          title={targetIssue}
          onChange={(event) => onUpdate({ target: event.target.value })}
        />
      </td>
      <td className="glossary-entry-category-cell">
        <DropdownField
          compact
          floating
          floatingLayer="dialog"
          label={t("settings.glossary.table.categoryAtRow", { row: position })}
          value={entry.category}
          options={categoryOptions}
          disabled={saving}
          onChange={(value) => onUpdate({ category: value as GlossaryEntry["category"] })}
        />
      </td>
      <td className="glossary-entry-toggle-cell">
        <button
          className="settings-switch-button"
          type="button"
          role="switch"
          aria-checked={entry.target === null}
          aria-label={t("settings.glossary.table.keepOriginalAtRow", { row: position })}
          disabled={saving}
          onClick={() => onUpdate({ target: entry.target === null ? "" : null })}
        >
          <span className="switch-track" aria-hidden="true"><span /></span>
        </button>
      </td>
      <td className="glossary-entry-toggle-cell">
        <button
          className="settings-switch-button"
          type="button"
          role="switch"
          aria-checked={entry.case_sensitive}
          aria-label={t("settings.glossary.table.caseSensitiveAtRow", { row: position })}
          disabled={saving}
          onClick={() => onUpdate({ case_sensitive: !entry.case_sensitive })}
        >
          <span className="switch-track" aria-hidden="true"><span /></span>
        </button>
      </td>
      <td className="glossary-entry-action-cell">
        <button
          className="api-row-icon-button api-danger-button"
          type="button"
          aria-label={t("settings.glossary.table.deleteRow", { row: position })}
          title={t("common.delete")}
          disabled={saving}
          onClick={onRemove}
        >
          <Trash2 size={15} aria-hidden="true" />
        </button>
      </td>
    </tr>
  );
});

function SortableHeader({
  label,
  sortKey,
  activeSortKey,
  direction,
  className,
  onSort,
}: {
  label: string;
  sortKey: GlossaryEntrySortKey;
  activeSortKey: GlossaryEntrySortKey;
  direction: GlossaryEntrySortDirection;
  className?: string;
  onSort: (sortKey: GlossaryEntrySortKey) => void;
}) {
  const active = activeSortKey === sortKey;
  return (
    <th
      className={className}
      scope="col"
      aria-sort={active ? (direction === "asc" ? "ascending" : "descending") : "none"}
    >
      <button type="button" onClick={() => onSort(sortKey)}>
        <span>{label}</span>
        {active && sortKey !== "position" && (
          direction === "asc" ? <ArrowUp size={12} aria-hidden="true" /> : <ArrowDown size={12} aria-hidden="true" />
        )}
      </button>
    </th>
  );
}

function SelectionCheckbox({
  checked,
  indeterminate = false,
  disabled,
  label,
  onChange,
}: {
  checked: boolean;
  indeterminate?: boolean;
  disabled: boolean;
  label: string;
  onChange: () => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = indeterminate;
  }, [indeterminate]);
  return (
    <label className="glossary-entry-checkbox-wrap">
      <input
        className="glossary-entry-checkbox"
        ref={ref}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        aria-label={label}
        onChange={onChange}
      />
    </label>
  );
}
