// `FR-SETUP-8`: "First-run shows a 4-step onboarding: Toolchain → Authenticate → Create or
// open a project → Plug in a board. Each step is skippable and re-enterable."
// `TOOLCHAIN-SETUP.md` §9: "Step ① and ② run concurrently with the user reading step ③" —
// `useDoctor` already probes everything up front regardless of which step is showing, so
// that's true here for free, not something this component has to orchestrate.

import { useState } from "react";
import type { ProjectEntry } from "../../lib/bindings";
import { settingsGetGlobal, settingsSetGlobal } from "../../lib/ipc";
import { emptyPatch } from "../../lib/settings";
import { strings } from "../../lib/strings";
import { useDeviceList } from "../devices/useDeviceList";
import { useDoctor } from "../doctor/useDoctor";
import { LauncherScreen } from "../launcher/LauncherScreen";

type Step = 1 | 2 | 3 | 4;

const primaryButton = "rounded bg-neutral-900 px-3 py-1.5 text-xs font-medium text-neutral-50 disabled:opacity-40 dark:bg-neutral-100 dark:text-neutral-900";
const secondaryButton = "rounded border border-neutral-300 px-3 py-1.5 text-xs font-medium hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800";

async function markCompleted() {
  await settingsSetGlobal({ ...emptyPatch(), onboardingCompleted: true });
}

function StepDots({ step }: { step: Step }) {
  return (
    <div className="mb-4 flex items-center gap-1.5" aria-hidden>
      {([1, 2, 3, 4] as Step[]).map((s) => (
        <span key={s} className={`h-1.5 w-6 rounded-full ${s <= step ? "bg-neutral-900 dark:bg-neutral-100" : "bg-neutral-200 dark:bg-neutral-800"}`} />
      ))}
    </div>
  );
}

function ToolchainStep({ onNext, onSkip }: { onNext: () => void; onSkip: () => void }) {
  const { report, loading, install } = useDoctor();
  const claude = report?.claudeBinary;
  const pio = report?.pioBinary;
  // Probing takes a moment (concurrent, timeout-bounded — `TOOLCHAIN-SETUP.md` §9) — show a
  // loading state rather than the "not found" / Install button while `report` is still
  // `null`, which otherwise flashes a misleading "please install this" for tools that are
  // actually already there.
  const detecting = loading && !report;
  return (
    <div>
      <h3 className="mb-2 text-sm font-semibold">{strings.onboarding.step1Title}</h3>
      <div className="mb-4 space-y-2 text-xs">
        {detecting ? (
          <p className="rounded border border-neutral-200 px-3 py-2 text-neutral-500 dark:border-neutral-800 dark:text-neutral-400">
            Detecting installed toolchain…
          </p>
        ) : (
          <>
            <div className="flex items-center justify-between rounded border border-neutral-200 px-3 py-2 dark:border-neutral-800">
              <span>Claude Code {claude?.status === "ok" ? `✓ ${claude.version}` : "✗ not found"}</span>
              {claude?.status !== "ok" && (
                <button type="button" onClick={() => void install("installClaude")} className={secondaryButton}>
                  Install
                </button>
              )}
            </div>
            <div className="flex items-center justify-between rounded border border-neutral-200 px-3 py-2 dark:border-neutral-800">
              <span>PlatformIO {pio?.status === "ok" ? `✓ ${pio.version}` : "✗ not found"}</span>
              {pio?.status !== "ok" && (
                <button type="button" onClick={() => void install("installPio")} className={secondaryButton}>
                  Install
                </button>
              )}
            </div>
          </>
        )}
      </div>
      <div className="flex justify-between">
        <button type="button" onClick={onSkip} className={secondaryButton}>
          {strings.onboarding.skip}
        </button>
        <button type="button" onClick={onNext} className={primaryButton}>
          {strings.onboarding.next}
        </button>
      </div>
    </div>
  );
}

function SignInStep({ onNext, onBack, onSkip }: { onNext: () => void; onBack: () => void; onSkip: () => void }) {
  const { report, loading, authenticate, refresh } = useDoctor();
  const auth = report?.claudeAuth;
  const detecting = loading && !report;
  return (
    <div>
      <h3 className="mb-2 text-sm font-semibold">{strings.onboarding.step2Title}</h3>
      <div className="mb-4 flex items-center justify-between rounded border border-neutral-200 px-3 py-2 text-xs dark:border-neutral-800">
        {detecting ? (
          <span className="text-neutral-500 dark:text-neutral-400">Checking sign-in status…</span>
        ) : (
          <>
            <span>Claude Code {auth?.status === "ok" ? "— signed in ✓" : "— not signed in"}</span>
            {auth?.status !== "ok" && (
              <button
                type="button"
                onClick={() => {
                  void authenticate();
                  setTimeout(() => void refresh(true), 3000);
                }}
                className={secondaryButton}
              >
                Sign in
              </button>
            )}
          </>
        )}
      </div>
      <div className="flex justify-between">
        <div className="flex gap-2">
          <button type="button" onClick={onBack} className={secondaryButton}>
            {strings.onboarding.back}
          </button>
          <button type="button" onClick={onSkip} className={secondaryButton}>
            {strings.onboarding.skip}
          </button>
        </div>
        <button type="button" onClick={onNext} className={primaryButton}>
          {strings.onboarding.next}
        </button>
      </div>
    </div>
  );
}

function ProjectStep({ onOpened, onBack, onSkip }: { onOpened: (p: ProjectEntry) => void; onBack: () => void; onSkip: () => void }) {
  return (
    <div>
      <h3 className="mb-2 text-sm font-semibold">{strings.onboarding.step3Title}</h3>
      <div className="mb-4 max-h-[55vh] overflow-y-auto rounded border border-neutral-200 dark:border-neutral-800">
        <LauncherScreen onOpenWorkspace={onOpened} />
      </div>
      <div className="flex justify-between">
        <button type="button" onClick={onBack} className={secondaryButton}>
          {strings.onboarding.back}
        </button>
        <button type="button" onClick={onSkip} className={secondaryButton}>
          {strings.onboarding.skip}
        </button>
      </div>
    </div>
  );
}

function BoardStep({ onFinish, onBack }: { onFinish: () => void; onBack: () => void }) {
  const { devices } = useDeviceList();
  return (
    <div>
      <h3 className="mb-2 text-sm font-semibold">{strings.onboarding.step4Title}</h3>
      <p className="mb-3 text-xs text-neutral-500 dark:text-neutral-400">Plug in your board — we'll detect it.</p>
      <div className="mb-4 space-y-1.5 text-xs">
        {devices.length === 0 && <p className="text-neutral-500 dark:text-neutral-400">No devices detected yet.</p>}
        {devices.map((d) => (
          <div key={d.port} className="rounded border border-neutral-200 px-3 py-2 dark:border-neutral-800">
            {d.port} — {d.knownBridge ?? d.description}
          </div>
        ))}
      </div>
      <div className="flex justify-between">
        <button type="button" onClick={onBack} className={secondaryButton}>
          {strings.onboarding.back}
        </button>
        <button type="button" onClick={onFinish} className={primaryButton}>
          {strings.onboarding.finish}
        </button>
      </div>
    </div>
  );
}

export function Onboarding({ onDone }: { onDone: (opened: ProjectEntry | null) => void }) {
  const [step, setStep] = useState<Step>(1);
  const [openedProject, setOpenedProject] = useState<ProjectEntry | null>(null);

  const finish = () => {
    void markCompleted();
    onDone(openedProject);
  };

  // Step 3 (project list) reads much better with more room — paths and board ids wrap
  // heavily at the same narrow width that suits the other, form-like steps.
  const containerWidth = step === 3 ? "max-w-4xl" : "max-w-lg";

  return (
    <div className={`mx-auto flex h-full w-full ${containerWidth} flex-col justify-center px-4 py-6`}>
      <h1 className="mb-1 text-lg font-semibold">{strings.onboarding.title}</h1>
      <StepDots step={step} />
      {step === 1 && <ToolchainStep onNext={() => setStep(2)} onSkip={finish} />}
      {step === 2 && <SignInStep onNext={() => setStep(3)} onBack={() => setStep(1)} onSkip={finish} />}
      {step === 3 && (
        <ProjectStep
          onOpened={(p) => {
            setOpenedProject(p);
            setStep(4);
          }}
          onBack={() => setStep(2)}
          onSkip={finish}
        />
      )}
      {step === 4 && <BoardStep onFinish={finish} onBack={() => setStep(3)} />}
    </div>
  );
}

/** Re-enterable from Doctor at any time — resets the persisted flag and lets the caller
 * decide when to actually show the wizard again. */
export async function resetOnboarding() {
  const settings = await settingsGetGlobal();
  await settingsSetGlobal({ ...emptyPatch(), onboardingCompleted: false });
  return settings;
}
