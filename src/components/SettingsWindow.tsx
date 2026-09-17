import { useCallback, useEffect, useMemo, useState } from "react";
import {
  BadgeCheck,
  Bot,
  Download,
  Eye,
  ExternalLink,
  FolderPlus,
  FolderOpen,
  Globe2,
  KeyRound,
  Lock,
  Maximize2,
  MessageCircle,
  Move,
  PawPrint,
  Play,
  Plus,
  RefreshCw,
  Rocket,
  Save,
  Search,
  Sparkles,
  Trash2,
  ZoomIn,
} from "lucide-react";
import {
  BASE_CELL,
  PET_STATES,
  resolveActivePetId,
  type PetPackage,
  type PetState,
} from "../lib/petContract";
import {
  DEFAULT_GALLERY_INDEX_URL,
  DEFAULT_SETTINGS,
  loadSettings,
  saveSettings,
  type AppSettings,
} from "../lib/settings";
import {
  APP_LATEST_RELEASE_URL,
  applyPetWindowSettings,
  checkForAppUpdate,
  choosePetFolder,
  importPetFromUrl,
  listPetPackages,
  notifyPetSettings,
  revealPetFolder,
  restoreAutostartPreference,
  writeAutostart,
  type GalleryIndex,
  type GalleryPet,
  type UpdateCheckResult,
} from "../lib/tauriApi";

const STATE_LABELS: Record<PetState, string> = {
  idle: "대기",
  "running-right": "오른쪽으로 달리기",
  "running-left": "왼쪽으로 달리기",
  waving: "손 흔들기",
  jumping: "점프",
  failed: "오류",
  waiting: "입력 대기",
  running: "작업 중",
  review: "검토 / 결과 확인",
};

const STATE_HINTS: Record<PetState, string> = {
  idle: "기본 상태입니다. ‘대기 동작 다양화’가 켜져 있으면 가끔 다른 동작을 짧게 재생합니다.",
  "running-right": "펫을 오른쪽으로 드래그할 때 자동으로 재생됩니다.",
  "running-left": "펫을 왼쪽으로 드래그할 때 자동으로 재생됩니다.",
  waving: "수동 선택하거나, AI 채팅을 열었을 때 인사 동작으로 재생됩니다.",
  jumping: "수동 선택하거나, AI 응답에 긍정적인 표현이 있을 때 재생됩니다.",
  failed: "수동 선택하거나, AI 채팅 오류 시 재생됩니다.",
  waiting: "수동 선택하거나, AI 채팅 입력 중 재생됩니다.",
  running: "수동 선택하거나, AI 응답을 기다리는 동안 재생됩니다.",
  review: "수동 선택하거나, AI 응답의 기본 완료 상태로 재생됩니다.",
};

type UpdateCheckStatus = "idle" | "checking" | "available" | "latest" | "error";

interface UpdateCheckState {
  status: UpdateCheckStatus;
  currentVersion: string;
  latestVersion: string;
  releaseUrl: string;
  message: string;
}

export function SettingsWindow() {
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [packages, setPackages] = useState<PetPackage[]>([]);
  const [galleryPets, setGalleryPets] = useState<GalleryPet[]>([]);
  const [gallerySearch, setGallerySearch] = useState("");
  const [galleryUrlDraft, setGalleryUrlDraft] = useState(DEFAULT_GALLERY_INDEX_URL);
  const [galleryLoading, setGalleryLoading] = useState(false);
  const [updateCheck, setUpdateCheck] = useState<UpdateCheckState>({
    status: "idle",
    currentVersion: "",
    latestVersion: "",
    releaseUrl: APP_LATEST_RELEASE_URL,
    message: "업데이트를 아직 확인하지 않았습니다.",
  });
  const [status, setStatus] = useState("준비됨");
  const [newPetFolder, setNewPetFolder] = useState("");

  const activePet = useMemo(
    () => packages.find((candidate) => candidate.id === resolveActivePetId(settings.activePetId, packages)),
    [packages, settings.activePetId],
  );
  const scalePercent = Math.round((settings.width / BASE_CELL.width) * 100);
  const filteredGalleryPets = useMemo(() => {
    const query = gallerySearch.trim().toLowerCase();
    if (!query) {
      return galleryPets;
    }
    return galleryPets.filter((pet) =>
      [pet.name, pet.displayName, pet.author, pet.description, ...(pet.tags ?? [])]
        .filter(Boolean)
        .join(" ")
        .toLowerCase()
        .includes(query),
    );
  }, [galleryPets, gallerySearch]);

  const refreshPackages = useCallback(async (petFolders: string[] = []) => {
    const found = await listPetPackages(petFolders);
    setPackages(found);
    return found;
  }, []);

  useEffect(() => {
    let cancelled = false;
    async function boot() {
      const loadedSettings = await loadSettings();
      const foundPackages = await refreshPackages(loadedSettings.petFolders);
      const autostart = await restoreAutostartPreference(loadedSettings.autostart);
      if (cancelled) {
        return;
      }
      const nextSettings = {
        ...loadedSettings,
        autostart,
        activePetId: resolveActivePetId(loadedSettings.activePetId, foundPackages),
      };
      setSettings(nextSettings);
      if (
        nextSettings.autostart !== loadedSettings.autostart ||
        nextSettings.activePetId !== loadedSettings.activePetId
      ) {
        void saveSettings(nextSettings);
      }
      setGalleryUrlDraft(loadedSettings.galleryIndexUrl);
      void loadGallery(loadedSettings.galleryIndexUrl, false);
      void checkUpdates(false);
    }
    void boot();
    return () => {
      cancelled = true;
    };
  }, [refreshPackages]);

  async function commit(
    next: AppSettings,
    message = "저장했습니다",
    patch?: Partial<AppSettings>,
  ) {
    setSettings(next);
    await saveSettings(next);
    if (patch && Object.keys(patch).length > 0) {
      await applyPetWindowSettings(patch);
    }
    await notifyPetSettings(next);
    setStatus(message);
  }

  async function update<K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
    message = "저장했습니다",
  ) {
    const next = { ...settings, [key]: value };
    const patchKeys: Array<keyof AppSettings> = [
      "width",
      "height",
      "x",
      "y",
      "alwaysOnTop",
      "clickThrough",
    ];
    await commit(next, message, patchKeys.includes(key) ? { [key]: value } : undefined);
  }

  async function setScale(percent: number) {
    const width = Math.round((BASE_CELL.width * percent) / 100);
    const height = Math.round((BASE_CELL.height * percent) / 100);
    const next = { ...settings, width, height };
    await commit(next, `크기를 ${percent}%로 변경했습니다`, { width, height });
  }

  async function setSize(width: number, height: number) {
    const next = { ...settings, width, height };
    await commit(next, "크기를 변경했습니다", { width, height });
  }

  async function setPosition(x: number, y: number) {
    const next = { ...settings, x, y, positionCoordinateSpace: "logical" as const };
    await commit(next, "위치를 변경했습니다", { x, y });
  }

  async function resetPosition() {
    await setPosition(80, 80);
  }

  async function refresh() {
    const found = await refreshPackages(settings.petFolders);
    const activePetId = resolveActivePetId(settings.activePetId, found);
    await commit({ ...settings, activePetId }, "펫 목록을 새로고침했습니다");
  }

  async function loadGallery(indexUrl = galleryUrlDraft, persist = true) {
    const trimmed = indexUrl.trim();
    if (!trimmed) {
      setStatus("갤러리 색인 주소를 입력하세요");
      return;
    }

    setGalleryLoading(true);
    try {
      if (persist && trimmed !== settings.galleryIndexUrl) {
        await commit({ ...settings, galleryIndexUrl: trimmed }, "갤러리 주소를 저장했습니다");
      }
      const response = await fetch(trimmed, { cache: "no-store" });
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      const index = (await response.json()) as GalleryIndex;
      setGalleryPets(Array.isArray(index.pets) ? index.pets : []);
      setStatus(`갤러리를 불러왔습니다: 펫 ${index.pets?.length ?? 0}개`);
    } catch (error) {
      console.error("Failed to load gallery", error);
      setStatus("갤러리를 불러오지 못했습니다. 주소 또는 네트워크를 확인하세요.");
    } finally {
      setGalleryLoading(false);
    }
  }

  async function importGalleryPet(pet: GalleryPet) {
    const downloadUrl = resolveGalleryUrl(pet.download, settings.galleryIndexUrl);
    if (!downloadUrl) {
      setStatus("이 펫에는 다운로드 주소가 없습니다");
      return;
    }

    try {
      setStatus(`${pet.displayName ?? pet.name} 가져오는 중`);
      const imported = await importPetFromUrl(downloadUrl);
      if (!imported) {
        setStatus("현재 미리보기 환경에서는 가져오기를 지원하지 않습니다");
        return;
      }
      const found = await refreshPackages(settings.petFolders);
      const activePetId = resolveActivePetId(imported.id, found);
      await commit({ ...settings, activePetId }, `${imported.displayName}을(를) 가져왔습니다`);
    } catch (error) {
      console.error("Failed to import gallery pet", error);
      setStatus("가져오기에 실패했습니다. 다운로드 링크가 ZIP 펫 패키지인지 확인하세요.");
    }
  }

  async function addPetFolder(folder = newPetFolder) {
    const trimmed = folder.trim();
    if (!trimmed) {
      setStatus("펫 폴더 경로를 입력하세요");
      return;
    }

    const petFolders = Array.from(new Set([...settings.petFolders, trimmed]));
    const found = await refreshPackages(petFolders);
    const activePetId = resolveActivePetId(settings.activePetId, found);
    await commit({ ...settings, petFolders, activePetId }, "펫 폴더를 추가했습니다");
    setNewPetFolder("");
  }

  async function chooseAndAddPetFolder() {
    const selected = await choosePetFolder();
    if (selected) {
      await addPetFolder(selected);
    }
  }

  async function removePetFolder(folder: string) {
    const petFolders = settings.petFolders.filter((candidate) => candidate !== folder);
    const found = await refreshPackages(petFolders);
    const activePetId = resolveActivePetId(settings.activePetId, found);
    await commit({ ...settings, petFolders, activePetId }, "펫 폴더를 제거했습니다");
  }

  async function toggleAutostart(enabled: boolean) {
    try {
      await writeAutostart(enabled);
      await commit(
        { ...settings, autostart: enabled },
        enabled ? "시작 프로그램을 켰습니다" : "시작 프로그램을 껐습니다",
      );
    } catch (error) {
      console.error("Failed to update autostart", error);
      setStatus("시작 프로그램 설정에 실패했습니다. 최신 버전 설치 여부를 확인하세요.");
    }
  }

  async function toggleDragging(enabled: boolean) {
    await commit(
      { ...settings, dragEnabled: enabled, locked: !enabled },
      enabled ? "펫 드래그를 허용했습니다" : "현재 위치를 고정했습니다",
    );
  }

  async function checkUpdates(manual = true) {
    setUpdateCheck((current) => ({
      ...current,
      status: "checking",
      message: "업데이트 확인 중...",
    }));

    try {
      const result = await checkForAppUpdate();
      const next = createUpdateCheckState(result);
      setUpdateCheck(next);
      if (manual) {
        setStatus(next.status === "available" ? `새 버전 ${next.latestVersion}을 찾았습니다` : "최신 버전입니다");
      }
    } catch (error) {
      console.error("Failed to check updates", error);
      setUpdateCheck((current) => ({
        ...current,
        status: "error",
        message: "자동 업데이트 확인에서 버전 정보를 받지 못했습니다. 배포 페이지에서 최신 버전을 확인하세요.",
      }));
      if (manual) {
        setStatus("업데이트 확인에 실패했습니다. 네트워크를 확인하세요.");
      }
    }
  }

  function scrollToPanel(id: string) {
    document.getElementById(id)?.scrollIntoView({ block: "start", behavior: "smooth" });
  }

  return (
    <main className="settings-shell">
      <aside className="settings-rail" aria-label="설정 탐색">
        <div className="brand-mark">
          <PawPrint size={24} />
        </div>
        <button
          className="rail-button is-active"
          type="button"
          onClick={() => scrollToPanel("pet-section")}
          title="펫"
        >
          <Sparkles size={20} />
        </button>
        <button
          className="rail-button"
          type="button"
          onClick={() => scrollToPanel("size-section")}
          title="크기"
        >
          <ZoomIn size={20} />
        </button>
        <button
          className="rail-button"
          type="button"
          onClick={() => scrollToPanel("motion-section")}
          title="동작"
        >
          <Play size={20} />
        </button>
        <button
          className="rail-button"
          type="button"
          onClick={() => scrollToPanel("chat-section")}
          title="대화"
        >
          <MessageCircle size={20} />
        </button>
        <button
          className="rail-button"
          type="button"
          onClick={() => scrollToPanel("gallery-section")}
          title="갤러리"
        >
          <Globe2 size={20} />
        </button>
      </aside>

      <section className="settings-main">
        <header className="settings-header">
          <div>
            <p className="eyebrow">Desktop Pet</p>
            <h1>데스크톱 펫 설정</h1>
          </div>
          <div className="status-pill">
            <Save size={16} />
            {status}
          </div>
        </header>

        <section className="hero-panel" id="size-section">
          <div className="hero-copy">
            <div className="panel-title">
              <ZoomIn size={20} />
              <h2>크기 조절</h2>
            </div>
            <div className="scale-readout">{scalePercent}%</div>
            <p>슬라이더를 드래그하면 원래 비율로 크기가 바뀝니다. 아래에서 정확한 너비와 높이도 조절할 수 있습니다.</p>
          </div>
          <div className="scale-controls">
            <input
              aria-label="펫 크기 조절"
              className="scale-slider"
              type="range"
              min="50"
              max="500"
              step="5"
              value={scalePercent}
              onChange={(event) => setScale(Number(event.target.value))}
            />
            <div className="scale-presets">
              {[75, 100, 150, 200, 300].map((percent) => (
                <button key={percent} type="button" onClick={() => setScale(percent)}>
                  {percent}%
                </button>
              ))}
            </div>
          </div>
        </section>

        <div className="settings-grid">
          <section className="panel pet-panel" id="pet-section">
            <div className="panel-title">
              <Sparkles size={18} />
              <h2>펫</h2>
            </div>
            <label className="field">
              <span>현재 펫</span>
              <select
                value={settings.activePetId ?? ""}
                onChange={(event) => update("activePetId", event.target.value, "펫을 변경했습니다")}
              >
                {packages.length === 0 ? <option value="">펫 패키지를 찾지 못했습니다</option> : null}
                {packages.map((pet) => (
                  <option key={`${pet.rootDir}-${pet.id}`} value={pet.id}>
                    {pet.displayName}
                  </option>
                ))}
              </select>
            </label>
            {activePet ? (
              <div className="pet-details">
                <strong>{activePet.displayName}</strong>
                <span>{activePet.description}</span>
                <div className="asset-badges">
                  <span>1x</span>
                  {activePet.spritesheets["2x"] ? <span>2x</span> : null}
                  {activePet.spritesheets["4x"] ? <span>4x</span> : null}
                </div>
              </div>
            ) : null}
            <div className="button-row">
              <button onClick={refresh} type="button">
                <RefreshCw size={16} />
                새로고침
              </button>
              <button
                onClick={() => activePet && revealPetFolder(activePet.rootDir)}
                type="button"
                disabled={!activePet}
              >
                <FolderOpen size={16} />
                폴더
              </button>
            </div>
          </section>

          <section className="panel pet-folders-panel">
            <div className="panel-title">
              <FolderPlus size={18} />
              <h2>펫 폴더</h2>
            </div>
            <div className="folder-picker">
              <label className="field">
                <span>사용자 지정 경로</span>
                <input
                  type="text"
                  value={newPetFolder}
                  placeholder="예: D:\\Pets 또는 ~/pets"
                  onChange={(event) => setNewPetFolder(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      void addPetFolder();
                    }
                  }}
                />
              </label>
              <div className="button-row folder-actions">
                <button type="button" onClick={() => addPetFolder()}>
                  <Plus size={16} />
                  추가
                </button>
                <button type="button" onClick={chooseAndAddPetFolder}>
                  <FolderOpen size={16} />
                  선택
                </button>
              </div>
            </div>
            <div className="folder-list">
              {settings.petFolders.length === 0 ? (
                <span className="folder-empty">추가된 사용자 지정 폴더가 없습니다</span>
              ) : (
                settings.petFolders.map((folder) => (
                  <div className="folder-item" key={folder}>
                    <span title={folder}>{folder}</span>
                    <button
                      type="button"
                      aria-label={`${folder} 제거`}
                      onClick={() => removePetFolder(folder)}
                    >
                      <Trash2 size={15} />
                    </button>
                  </div>
                ))
              )}
            </div>
          </section>

          <section className="panel gallery-panel" id="gallery-section">
            <div className="panel-title">
              <Globe2 size={18} />
              <h2>온라인 갤러리</h2>
            </div>
            <div className="gallery-controls">
              <label className="field">
                <span>색인 주소</span>
                <input
                  type="url"
                  value={galleryUrlDraft}
                  placeholder={DEFAULT_GALLERY_INDEX_URL}
                  onChange={(event) => setGalleryUrlDraft(event.target.value)}
                />
              </label>
              <label className="field">
                <span>검색</span>
                <input
                  type="search"
                  value={gallerySearch}
                  placeholder="이름, 제작자, 태그"
                  onChange={(event) => setGallerySearch(event.target.value)}
                />
              </label>
              <button type="button" onClick={() => loadGallery()} disabled={galleryLoading}>
                <Search size={16} />
                {galleryLoading ? "불러오는 중" : "갤러리 불러오기"}
              </button>
            </div>
            <div className="gallery-list">
              {filteredGalleryPets.length === 0 ? (
                <span className="folder-empty">표시할 펫이 없습니다</span>
              ) : (
                filteredGalleryPets.map((pet) => (
                  <article className="gallery-pet-card" key={`${pet.id}-${pet.version}`}>
                    <img
                      src={resolveGalleryUrl(pet.previewImage ?? pet.preview, settings.galleryIndexUrl)}
                      alt=""
                    />
                    <div>
                      <strong>{pet.displayName ?? pet.name}</strong>
                      <span>{pet.description}</span>
                      <small>
                        {pet.author} · {pet.resolution} · {formatBytes(pet.downloadSize)}
                      </small>
                    </div>
                    <button type="button" onClick={() => importGalleryPet(pet)}>
                      <Download size={16} />
                      가져오기
                    </button>
                  </article>
                ))
              )}
            </div>
          </section>

          <section className="panel">
            <div className="panel-title">
              <Maximize2 size={18} />
              <h2>정확한 크기</h2>
            </div>
            <ToggleRow
              label="원래 비율 유지"
              value={settings.keepAspectRatio}
              onChange={(value) => update("keepAspectRatio", value, "비율 잠금을 변경했습니다")}
            />
            <div className="split-fields">
              <NumberField
                label="너비"
                value={settings.width}
                min={96}
                max={1200}
                onChange={(value) => {
                  const height = settings.keepAspectRatio
                    ? Math.round((value / BASE_CELL.width) * BASE_CELL.height)
                    : settings.height;
                  setSize(value, height);
                }}
              />
              <NumberField
                label="높이"
                value={settings.height}
                min={104}
                max={1300}
                onChange={(value) => {
                  const width = settings.keepAspectRatio
                    ? Math.round((value / BASE_CELL.height) * BASE_CELL.width)
                    : settings.width;
                  setSize(width, value);
                }}
              />
            </div>
          </section>

          <section className="panel">
            <div className="panel-title">
              <Move size={18} />
              <h2>위치</h2>
            </div>
            <div className="split-fields">
              <NumberField
                label="X"
                value={settings.x ?? 80}
                min={-4000}
                max={4000}
                onChange={(value) => setPosition(value, settings.y ?? 80)}
              />
              <NumberField
                label="Y"
                value={settings.y ?? 80}
                min={-4000}
                max={4000}
                onChange={(value) => setPosition(settings.x ?? 80, value)}
              />
            </div>
            <button className="wide-button" onClick={resetPosition} type="button">
              <Move size={16} />
              왼쪽 위로 이동
            </button>
          </section>

          <section className="panel" id="motion-section">
            <div className="panel-title">
              <Play size={18} />
              <h2>동작</h2>
            </div>
            <label className="field">
              <span>현재 동작</span>
              <select
                value={settings.manualState}
                onChange={(event) =>
                  update("manualState", event.target.value as PetState, "동작을 변경했습니다")
                }
              >
                {PET_STATES.map((state) => (
                  <option key={state} value={state}>
                    {STATE_LABELS[state]}
                  </option>
                ))}
              </select>
            </label>
            <p className="panel-note">{STATE_HINTS[settings.manualState]}</p>
            <label className="slider-field">
              <span>속도</span>
              <input
                type="range"
                min="0.25"
                max="3"
                step="0.05"
                value={settings.animationSpeed}
                onChange={(event) =>
                  update("animationSpeed", Number(event.target.value), "속도를 변경했습니다")
                }
              />
              <output>{settings.animationSpeed.toFixed(2)}x</output>
            </label>
            <ToggleRow
              label="대기 동작 다양화"
              value={settings.idleVariety}
              onChange={(value) => update("idleVariety", value, "대기 동작을 변경했습니다")}
            />
            <ToggleRow
              label="동작 줄이기"
              value={settings.reducedMotion}
              onChange={(value) => update("reducedMotion", value, "동작 설정을 변경했습니다")}
            />
            <ToggleRow
              label="픽셀 스타일 렌더링"
              value={settings.pixelated}
              onChange={(value) => update("pixelated", value, "렌더링 방식을 변경했습니다")}
            />
          </section>

          <section className="panel llm-panel" id="chat-section">
            <div className="panel-title">
              <Bot size={18} />
              <h2>AI 대화</h2>
            </div>
            <ToggleRow
              label="펫 대화 사용"
              value={settings.llmChatEnabled}
              onChange={(value) =>
                update("llmChatEnabled", value, value ? "대화 버튼을 표시했습니다" : "대화를 껐습니다")
              }
            />
            <p className="panel-note">
              켜면 펫 옆에 대화 버튼이 나타납니다. OpenAI 호환 API를 사용하며, 로컬 모델은 API 키가 없어도 됩니다.
            </p>
            <label className="field">
              <span>API 주소</span>
              <input
                type="url"
                value={settings.llmEndpoint}
                placeholder="예: https://api.example.com/v1"
                onChange={(event) => update("llmEndpoint", event.target.value, "API 주소를 저장했습니다")}
              />
            </label>
            <div className="split-fields">
              <label className="field">
                <span>모델</span>
                <input
                  type="text"
                  value={settings.llmModel}
                  placeholder="예: gpt-4.1-mini / qwen-plus"
                  onChange={(event) => update("llmModel", event.target.value, "모델을 저장했습니다")}
                />
              </label>
              <label className="field">
                <span>
                  <KeyRound size={13} />
                  API Key
                </span>
                <input
                  type="password"
                  value={settings.llmApiKey}
                  placeholder="로컬에 저장됨"
                  onChange={(event) => update("llmApiKey", event.target.value, "API 키를 저장했습니다")}
                />
              </label>
            </div>
            <label className="slider-field">
              <span>온도</span>
              <input
                type="range"
                min="0"
                max="2"
                step="0.05"
                value={settings.llmTemperature}
                onChange={(event) =>
                  update("llmTemperature", Number(event.target.value), "온도를 변경했습니다")
                }
              />
              <output>{settings.llmTemperature.toFixed(2)}</output>
            </label>
            <label className="field">
              <span>펫 말투</span>
              <textarea
                rows={4}
                value={settings.llmSystemPrompt}
                onChange={(event) =>
                  update("llmSystemPrompt", event.target.value, "펫 말투를 저장했습니다")
                }
              />
            </label>
          </section>

          <section className="panel">
            <div className="panel-title">
              <Lock size={18} />
              <h2>동작 설정</h2>
            </div>
            <ToggleRow
              label="항상 위에 표시"
              value={settings.alwaysOnTop}
              onChange={(value) => update("alwaysOnTop", value, "항상 위 표시를 변경했습니다")}
            />
            <ToggleRow
              label="펫 드래그 허용"
              value={settings.dragEnabled && !settings.locked}
              onChange={toggleDragging}
            />
            <ToggleRow
              label="마우스 클릭 통과"
              value={settings.clickThrough}
              onChange={(value) => update("clickThrough", value, "마우스 클릭 통과를 변경했습니다")}
            />
            <ToggleRow
              label="시작 시 표시"
              value={settings.showOnStartup}
              onChange={(value) => update("showOnStartup", value, "시작 시 표시를 변경했습니다")}
            />
            <ToggleRow
              label="Windows 시작 시 실행"
              value={settings.autostart}
              onChange={toggleAutostart}
              icon={<Rocket size={16} />}
            />
          </section>

          <section className={`panel update-panel is-${updateCheck.status}`}>
            <div className="panel-title">
              <BadgeCheck size={18} />
              <h2>업데이트</h2>
            </div>
            <div className="update-card">
              <strong>
                {updateCheck.status === "available"
                  ? `새 버전 ${updateCheck.latestVersion}을 찾았습니다`
                  : updateCheck.status === "latest"
                    ? "최신 버전입니다"
                    : updateCheck.status === "checking"
                      ? "업데이트 확인 중"
                      : updateCheck.status === "error"
                        ? "지금은 업데이트를 확인할 수 없습니다"
                        : "업데이트 확인"}
              </strong>
              <span>{updateCheck.message}</span>
              <small>
                현재 버전 {updateCheck.currentVersion || "알 수 없음"}
                {updateCheck.latestVersion ? ` · 최신 버전 ${updateCheck.latestVersion}` : ""}
              </small>
            </div>
            <div className="button-row">
              <button
                type="button"
                onClick={() => checkUpdates()}
                disabled={updateCheck.status === "checking"}
              >
                <RefreshCw size={16} />
                {updateCheck.status === "checking" ? "확인 중" : "업데이트 확인"}
              </button>
              <a
                className="settings-link-button"
                href={updateCheck.releaseUrl}
                target="_blank"
                rel="noreferrer"
              >
                <ExternalLink size={16} />
                배포 페이지
              </a>
            </div>
          </section>

          <section className="panel compact-panel">
            <div className="panel-title">
              <Eye size={18} />
              <h2>현재 렌더링</h2>
            </div>
            <div className="render-facts">
              <span>{settings.width} x {settings.height}</span>
              <span>{STATE_LABELS[settings.manualState]}</span>
              <span>{settings.pixelated ? "픽셀" : "부드럽게"}</span>
            </div>
          </section>
        </div>
      </section>
    </main>
  );
}

interface NumberFieldProps {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}

function createUpdateCheckState(result: UpdateCheckResult): UpdateCheckState {
  if (!result.checked) {
    return {
      status: "error",
      currentVersion: result.currentVersion,
      latestVersion: "",
      releaseUrl: result.releaseUrl,
      message: "자동 업데이트 확인에서 버전 정보를 받지 못했습니다. 배포 페이지에서 최신 버전을 확인하세요.",
    };
  }

  if (result.updateAvailable) {
    return {
      status: "available",
      currentVersion: result.currentVersion,
      latestVersion: result.latestVersion,
      releaseUrl: result.releaseUrl,
      message: "새 버전이 배포되었습니다. 아래 ‘배포 페이지’에서 설치 파일을 받으세요.",
    };
  }

  return {
    status: "latest",
    currentVersion: result.currentVersion,
    latestVersion: result.latestVersion,
    releaseUrl: result.releaseUrl,
    message: "최신 버전을 사용 중입니다.",
  };
}

function NumberField({ label, value, min, max, onChange }: NumberFieldProps) {
  return (
    <label className="field">
      <span>{label}</span>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </label>
  );
}

interface ToggleRowProps {
  label: string;
  value: boolean;
  icon?: React.ReactNode;
  onChange: (value: boolean) => void;
}

function ToggleRow({ label, value, icon, onChange }: ToggleRowProps) {
  return (
    <label className="toggle-row">
      <span>
        {icon}
        {label}
      </span>
      <input type="checkbox" checked={value} onChange={(event) => onChange(event.target.checked)} />
    </label>
  );
}

function resolveGalleryUrl(value: string | undefined, indexUrl: string): string {
  if (!value) {
    return "";
  }
  try {
    return new URL(value, new URL(".", indexUrl)).href;
  } catch {
    return value;
  }
}

function formatBytes(value?: number): string {
  if (!value) {
    return "알 수 없는 크기";
  }
  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
