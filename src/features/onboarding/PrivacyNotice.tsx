// `NFR-S2`/`M10`: "A first-run notice states plainly that prompts, and the code Claude
// reads, are sent to Anthropic by the Claude CLI." Gated on `privacyNoticeAcknowledged`
// rather than folded into the 4-step onboarding wizard (`TOOLCHAIN-SETUP.md` §9's step
// count is spec'd at exactly four) — this is a one-time disclosure the user acknowledges,
// not a step they can skip and re-enter from Doctor.

import { settingsSetGlobal } from "../../lib/ipc";
import { emptyPatch } from "../../lib/settings";
import { strings } from "../../lib/strings";

export function PrivacyNotice({ onAcknowledged }: { onAcknowledged: () => void }) {
  const acknowledge = async () => {
    await settingsSetGlobal({ ...emptyPatch(), privacyNoticeAcknowledged: true });
    onAcknowledged();
  };

  return (
    <div className="mx-auto flex h-full max-w-lg flex-col justify-center px-4 py-6">
      <h1 className="mb-3 text-lg font-semibold">{strings.privacyNotice.title}</h1>
      <p className="mb-6 text-sm text-neutral-600 dark:text-neutral-400">{strings.privacyNotice.body}</p>
      <button
        type="button"
        onClick={() => void acknowledge()}
        className="self-start rounded bg-neutral-900 px-3 py-1.5 text-xs font-medium text-neutral-50 dark:bg-neutral-100 dark:text-neutral-900"
      >
        {strings.privacyNotice.acknowledge}
      </button>
    </div>
  );
}
