import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { Layout } from "./components/Layout";
import { LoginPage } from "./features/auth/LoginPage";
import { sessionStore } from "./features/auth/session";
import { DashboardPage } from "./features/dashboard/DashboardPage";
import { LogsPage } from "./features/logs/LogsPage";
import { MemoryPage } from "./features/memory/MemoryPage";
import { MetaPage } from "./features/meta/MetaPage";
import { SkillsPage } from "./features/skills/SkillsPage";
import { useStore } from "./hooks/useStore";

export function App() {
  const session = useStore(sessionStore);
  if (!session) return <LoginPage />;

  return (
    <HashRouter>
      <Routes>
        <Route element={<Layout />}>
          <Route path="/dashboard" element={<DashboardPage />} />
          <Route path="/memory" element={<MemoryPage />} />
          <Route path="/logs" element={<LogsPage />} />
          <Route path="/skills" element={<SkillsPage />} />
          <Route path="/meta" element={<MetaPage />} />
          <Route path="*" element={<Navigate to="/dashboard" replace />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
