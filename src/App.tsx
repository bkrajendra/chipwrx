import { useEffect, useState } from "react";
import { DoctorScreen } from "./features/doctor/DoctorScreen";
import { Onboarding } from "./features/onboarding/Onboarding";
import { LauncherScreen } from "./features/launcher/LauncherScreen";
import { Modal } from "./features/shell/Modal";
import { Workspace } from "./features/shell/Workspace";
import { useTheme } from "./features/shell/useTheme";
import type { ProjectEntry } from "./lib/bindings";
import { projectList, settingsGetGlobal } from "./lib/ipc";

/** `FR-UI-9`: a window opened via "Open in new window" is told which workspace to show
 * through the URL rather than starting at the Launcher — every window is the same
 * frontend bundle, sharing the same Rust core (and so the same global `PortBroker`). */
function workspaceIdFromUrl(): string | null {
  return new URLSearchParams(window.location.search).get("workspace");
}

function App() {
  useTheme();
  const [workspace, setWorkspace] = useState<ProjectEntry | null>(null);
  const [onboardingDone, setOnboardingDone] = useState<boolean | null>(null);
  const [resolvingUrlWorkspace, setResolvingUrlWorkspace] = useState(true);
  const [doctorOpen, setDoctorOpen] = useState(false);

  useEffect(() => {
    const id = workspaceIdFromUrl();
    if (!id) {
      setResolvingUrlWorkspace(false);
      return;
    }
    void projectList().then((projects) => {
      const found = projects.find((p) => p.id === id);
      if (found) setWorkspace(found);
      setResolvingUrlWorkspace(false);
    });
  }, []);

  useEffect(() => {
    void settingsGetGlobal().then((s) => setOnboardingDone(s.onboardingCompleted));
  }, []);

  if (resolvingUrlWorkspace || onboardingDone === null) {
    return <main className="flex h-screen w-screen items-center justify-center bg-neutral-50 text-sm text-neutral-500 dark:bg-neutral-950 dark:text-neutral-400">Loading…</main>;
  }

  if (workspace) {
    return (
      <main className="h-screen w-screen bg-neutral-50 text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
        <Workspace project={workspace} onBack={() => setWorkspace(null)} />
      </main>
    );
  }

  if (!onboardingDone) {
    return (
      <main className="flex h-screen w-screen flex-col bg-neutral-50 text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
        <Onboarding
          onDone={(opened) => {
            setOnboardingDone(true);
            if (opened) setWorkspace(opened);
          }}
        />
      </main>
    );
  }

  return (
    <main className="flex h-screen w-screen flex-col bg-neutral-50 text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
      <header className="flex items-center justify-between border-b border-neutral-200 px-4 py-3 dark:border-neutral-800">
        <h1 className="text-sm font-semibold tracking-wide">Vibe Hardware</h1>
        <button
          type="button"
          onClick={() => setDoctorOpen(true)}
          className="rounded border border-neutral-300 px-2.5 py-1 text-xs font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
        >
          Doctor
        </button>
      </header>
      <section className="flex-1 overflow-y-auto">
        <LauncherScreen onOpenWorkspace={setWorkspace} />
      </section>
      <Modal open={doctorOpen} onClose={() => setDoctorOpen(false)} label="Doctor">
        <DoctorScreen />
      </Modal>
    </main>
  );
}

export default App;
