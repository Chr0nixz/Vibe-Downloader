import { getCurrentWindow } from "@tauri-apps/api/window";

export async function startWindowDrag(): Promise<void> {
  await getCurrentWindow().startDragging();
}

export async function minimizeWindow(): Promise<void> {
  await getCurrentWindow().minimize();
}

export async function toggleMaximizeWindow(): Promise<void> {
  await getCurrentWindow().toggleMaximize();
}

export async function closeWindow(): Promise<void> {
  await getCurrentWindow().close();
}
