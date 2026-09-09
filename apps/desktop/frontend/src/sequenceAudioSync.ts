import type { GuiDocumentRequest, SequenceAudio } from "./types";

export function sequenceAudioKey(projectEpoch: number, path: string, objectKey: string | null, audio: SequenceAudio | null, durationSeconds: number): string {
  if (audio === null) return JSON.stringify({ projectEpoch, path, objectKey, durationSeconds });
  return JSON.stringify({ projectEpoch, path, objectKey, importPath: audio.import, resolvedPath: audio.resolvedPath, exists: audio.exists });
}

type AudioTarget = { key: string; request: GuiDocumentRequest };

/** Owns native audio transitions across editor effects and remounts. */
export class SequenceAudioSync {
  private desired: AudioTarget | null = null;
  private loaded: { key: string | null } | null = { key: null };
  private pending: Promise<void> = Promise.resolve();

  constructor(private readonly apply: (request: GuiDocumentRequest | null) => Promise<boolean>) {}

  synchronize(target: AudioTarget | null): Promise<void> {
    this.desired = target;
    const operation = this.pending.then(async () => {
      if (this.desired !== target || this.loaded?.key === (target?.key ?? null)) return;
      this.loaded = null;
      if (await this.apply(target?.request ?? null)) this.loaded = { key: target?.key ?? null };
    });
    // Each caller receives its error; a rejected operation must not block later navigation.
    this.pending = operation.catch(() => {});
    return operation;
  }
}
