// `FR-INI-2/3`: the schema-driven Form tab. Widgets map from the option's `type` ×
// `multiple` (see `SPEC.md` FR-INI-2's table); each field shows PlatformIO's own
// `description` as help text and an "inherited from [env]" badge with an "override here"
// action when the section's *effective* value isn't declared right here.

import { useMemo, useState } from "react";
import type { IniDocument, IniEdit, IniOptionSchema, IniSection } from "../../lib/bindings";

function widgetFor(option: IniOptionSchema): "text" | "textlist" | "number" | "select" | "chips" | "switch" {
  if (option.type === "boolean") return "switch";
  if (option.type === "choice") return option.multiple ? "chips" : "select";
  if (option.type === "integer" || option.type === "float") return "number";
  return option.multiple ? "textlist" : "text";
}

function OptionField({
  option,
  declaredValues,
  effectiveValues,
  inheritedFrom,
  onChange,
}: {
  option: IniOptionSchema;
  declaredValues: string[] | undefined;
  effectiveValues: string[] | undefined;
  inheritedFrom: string | null | undefined;
  onChange: (values: string[]) => void;
}) {
  const widget = widgetFor(option);
  const isDeclared = declaredValues !== undefined;
  const displayValues = declaredValues ?? effectiveValues ?? [];
  const placeholder = option.default != null ? String(option.default) : "";

  return (
    <div className="border-b border-neutral-100 py-2 dark:border-neutral-900">
      <div className="flex items-center justify-between gap-2">
        <label className="font-mono text-xs font-medium">{option.name}</label>
        {inheritedFrom && (
          <span className="rounded-full bg-neutral-100 px-2 py-0.5 text-[10px] text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400">
            inherited from [{inheritedFrom}]
          </span>
        )}
      </div>
      {option.description && <p className="mt-0.5 text-[11px] text-neutral-500 dark:text-neutral-400">{option.description}</p>}

      <div className="mt-1.5 flex items-start gap-2">
        {widget === "switch" && (
          <input
            type="checkbox"
            checked={displayValues[0] === "true" || displayValues[0] === "yes" || displayValues[0] === "1"}
            onChange={(e) => onChange([e.target.checked ? "true" : "false"])}
            className="mt-0.5"
          />
        )}
        {widget === "select" && (
          <select
            value={displayValues[0] ?? ""}
            onChange={(e) => onChange(e.target.value ? [e.target.value] : [])}
            className="w-full rounded border border-neutral-300 bg-white px-2 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
          >
            <option value="">{placeholder || "(unset)"}</option>
            {option.choices?.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        )}
        {widget === "chips" && (
          <div className="flex flex-wrap gap-1">
            {option.choices?.map((c) => {
              const active = displayValues.includes(c);
              return (
                <button
                  key={c}
                  type="button"
                  onClick={() => onChange(active ? displayValues.filter((v) => v !== c) : [...displayValues, c])}
                  className={`rounded-full border px-2 py-0.5 text-[11px] ${
                    active
                      ? "border-neutral-900 bg-neutral-900 text-neutral-50 dark:border-neutral-100 dark:bg-neutral-100 dark:text-neutral-900"
                      : "border-neutral-300 dark:border-neutral-700"
                  }`}
                >
                  {c}
                </button>
              );
            })}
          </div>
        )}
        {widget === "number" && (
          <input
            type="number"
            min={option.min ?? undefined}
            max={option.max ?? undefined}
            value={displayValues[0] ?? ""}
            placeholder={placeholder}
            onChange={(e) => onChange(e.target.value ? [e.target.value] : [])}
            className="w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs dark:border-neutral-700"
          />
        )}
        {widget === "text" && (
          <input
            type="text"
            value={displayValues[0] ?? ""}
            placeholder={placeholder}
            onChange={(e) => onChange(e.target.value ? [e.target.value] : [])}
            className="w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs dark:border-neutral-700"
          />
        )}
        {widget === "textlist" && (
          <textarea
            value={displayValues.join("\n")}
            placeholder={placeholder}
            onChange={(e) => onChange(e.target.value.split("\n").map((l) => l.trim()).filter(Boolean))}
            rows={Math.min(6, Math.max(2, displayValues.length))}
            className="w-full rounded border border-neutral-300 bg-transparent px-2 py-1 font-mono text-xs dark:border-neutral-700"
          />
        )}
        {!isDeclared && effectiveValues && (
          <button
            type="button"
            onClick={() => onChange(effectiveValues)}
            className="shrink-0 whitespace-nowrap rounded border border-neutral-300 px-2 py-1 text-[11px] hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            Override here
          </button>
        )}
      </div>
    </div>
  );
}

export function FormTab({
  document,
  schema,
  onApply,
}: {
  document: IniDocument;
  schema: IniOptionSchema[];
  onApply: (edits: IniEdit[]) => void;
}) {
  const sectionNames = useMemo(() => document.sections.map((s) => s.name), [document]);
  const [selected, setSelected] = useState(sectionNames[0] ?? "");
  const section: IniSection | undefined = document.sections.find((s) => s.name === selected) ?? document.sections[0];

  const scope = section?.name === "platformio" ? "platformio" : "env";
  const relevantOptions = useMemo(() => schema.filter((o) => o.scope === scope), [schema, scope]);
  const [group, setGroup] = useState<string>("all");
  const groups = useMemo(() => ["all", ...Array.from(new Set(relevantOptions.map((o) => o.group)))], [relevantOptions]);
  const visibleOptions = group === "all" ? relevantOptions : relevantOptions.filter((o) => o.group === group);

  if (!section) return <p className="p-4 text-xs text-neutral-500 dark:text-neutral-400">No sections in this file yet.</p>;

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
        <select
          value={section.name}
          onChange={(e) => setSelected(e.target.value)}
          className="rounded border border-neutral-300 bg-white px-2 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
        >
          {sectionNames.map((n) => (
            <option key={n} value={n}>
              [{n}]
            </option>
          ))}
        </select>
        <select
          value={group}
          onChange={(e) => setGroup(e.target.value)}
          className="rounded border border-neutral-300 bg-white px-2 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
        >
          {groups.map((g) => (
            <option key={g} value={g}>
              {g}
            </option>
          ))}
        </select>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4">
        {visibleOptions.map((option) => {
          const declared = section.declared.find((e) => e.name === option.name);
          const effective = section.effective.find((e) => e.name === option.name);
          return (
            <OptionField
              key={option.key}
              option={option}
              declaredValues={declared?.values}
              effectiveValues={effective?.values}
              inheritedFrom={effective?.inheritedFrom}
              onChange={(values) => onApply([{ type: "set", data: { section: section.name, name: option.name, values } }])}
            />
          );
        })}
      </div>
    </div>
  );
}
