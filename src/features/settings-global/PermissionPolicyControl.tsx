// `FR-CHAT-4`: three user-selectable permission policies, defaulting to Guarded.
// Unrestricted is "gated behind a typed confirmation that names the workspace path" — this
// control is mounted from within a workspace (the Chat header) specifically so that
// confirmation has a concrete path to show and match against, and edits the *global*
// setting (there's no per-project override UI yet — `ProjectClaudeSettings.
// permissionPolicy` exists in the data model but M4 doesn't add `settings_set_project`).

import { useEffect, useState } from "react";
import type { GlobalSettings, PermissionPolicySetting } from "../../lib/bindings";
import { settingsGetGlobal, settingsSetGlobal } from "../../lib/ipc";
import { emptyPatch } from "../../lib/settings";

const LABELS: Record<PermissionPolicySetting, string> = {
  guarded: "Guarded",
  assisted: "Assisted",
  unrestricted: "Unrestricted",
};

export function PermissionPolicyControl({ workspacePath }: { workspacePath: string }) {
  const [settings, setSettings] = useState<GlobalSettings | null>(null);
  const [pendingUnrestricted, setPendingUnrestricted] = useState(false);
  const [confirmText, setConfirmText] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void settingsGetGlobal().then(setSettings);
  }, []);

  const applyPolicy = async (policy: PermissionPolicySetting) => {
    if (!settings) return;
    setError(null);
    if (policy === "unrestricted") {
      setPendingUnrestricted(true);
      return;
    }
    setSettings(await settingsSetGlobal({ ...emptyPatch(), claude: { ...settings.claude, permissionPolicy: policy } }));
  };

  const cancelUnrestricted = () => {
    setPendingUnrestricted(false);
    setConfirmText("");
    setError(null);
  };

  const confirmUnrestricted = async () => {
    if (!settings) return;
    if (confirmText.trim() !== workspacePath) {
      setError("That doesn't match — type the workspace path exactly to confirm.");
      return;
    }
    setSettings(
      await settingsSetGlobal({
        ...emptyPatch(),
        claude: { ...settings.claude, permissionPolicy: "unrestricted" },
        advanced: { ...settings.advanced, allowUnrestrictedPolicy: true },
      }),
    );
    cancelUnrestricted();
  };

  if (!settings) return null;

  return (
    <div className="relative">
      <select
        aria-label="Permission policy"
        value={settings.claude.permissionPolicy}
        onChange={(e) => void applyPolicy(e.target.value as PermissionPolicySetting)}
        className="rounded border border-neutral-300 bg-white px-2 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
      >
        {(Object.keys(LABELS) as PermissionPolicySetting[]).map((p) => (
          <option key={p} value={p}>
            {LABELS[p]}
          </option>
        ))}
      </select>

      {pendingUnrestricted && (
        <div className="absolute right-0 top-full z-10 mt-1 w-80 rounded border border-red-400 bg-white p-3 text-xs shadow-lg dark:border-red-700 dark:bg-neutral-900">
          <p className="mb-2 font-medium text-red-600 dark:text-red-400">
            Unrestricted skips every permission check for turns in this workspace — Claude can run any command with no confirmation.
          </p>
          <p className="mb-1 text-neutral-600 dark:text-neutral-400">Type the workspace path to confirm:</p>
          <p className="mb-2 break-all rounded bg-neutral-100 px-2 py-1 font-mono dark:bg-neutral-800">{workspacePath}</p>
          <input
            value={confirmText}
            onChange={(e) => setConfirmText(e.target.value)}
            className="mb-2 w-full rounded border border-neutral-300 bg-transparent px-2 py-1 dark:border-neutral-700"
            autoFocus
          />
          {error && <p className="mb-2 text-red-500">{error}</p>}
          <div className="flex justify-end gap-2">
            <button type="button" onClick={cancelUnrestricted} className="rounded border border-neutral-300 px-2 py-1 dark:border-neutral-700">
              Cancel
            </button>
            <button type="button" onClick={() => void confirmUnrestricted()} className="rounded bg-red-600 px-2 py-1 font-medium text-white">
              Enable Unrestricted
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
