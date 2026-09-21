import { useState } from "react";
import { ChatScreen } from "./features/chat/ChatScreen";
import { DoctorScreen } from "./features/doctor/DoctorScreen";
import { LauncherScreen } from "./features/launcher/LauncherScreen";
import type { ProjectEntry } from "./lib/bindings";

type Tab = "projects" | "doctor";

function App() {
  const [tab, setTab] = useState<Tab>("projects");
  const [workspace, setWorkspace] = useState<ProjectEntry | null>(null);

  if (workspace) {
    return (
      <main className="flex h-screen w-screen flex-col bg-neutral-50 text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
        <div className="flex items-center gap-2 border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
          <button
            type="button"
            onClick={() => setWorkspace(null)}
            className="text-xs text-neutral-500 hover:underline dark:text-neutral-400"
          >
            ← Projects
          </button>
        </div>
        <section className="flex-1 overflow-hidden">
          <ChatScreen workspaceId={workspace.id} workspaceName={workspace.name} workspacePath={workspace.path} />
        </section>
      </main>
    );
  }

  return (
    <main className="flex h-screen w-screen flex-col bg-neutral-50 text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
      <header className="flex items-center justify-between border-b border-neutral-200 px-4 py-3 dark:border-neutral-800">
        <h1 className="text-sm font-semibold tracking-wide">Vibe Hardware</h1>
        <nav className="flex gap-1">
          {(["projects", "doctor"] as const).map((t) => (
            <button
              key={t}
              type="button"
              onClick={() => setTab(t)}
              className={`rounded px-2.5 py-1 text-xs font-medium capitalize ${
                tab === t
                  ? "bg-neutral-200 dark:bg-neutral-800"
                  : "text-neutral-500 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-900"
              }`}
            >
              {t}
            </button>
          ))}
        </nav>
      </header>

      <section className="flex-1 overflow-y-auto">
        {tab === "projects" ? <LauncherScreen onOpenWorkspace={setWorkspace} /> : <DoctorScreen />}
      </section>
    </main>
  );
}

export default App;
