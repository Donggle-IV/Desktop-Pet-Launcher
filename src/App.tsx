import { ErrorBoundary } from "./components/ErrorBoundary";
import { PetWindow } from "./components/PetWindow";
import { SettingsWindow } from "./components/SettingsWindow";

export function App() {
  const route = window.location.hash.replace("#", "") || "pet";
  return (
    <ErrorBoundary fallbackTitle={route === "settings" ? "설정 화면 오류" : "데스크톱 펫 화면 오류"}>
      {route === "settings" ? <SettingsWindow /> : <PetWindow />}
    </ErrorBoundary>
  );
}
