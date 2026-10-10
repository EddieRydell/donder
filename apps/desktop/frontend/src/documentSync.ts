import type { AppSnapshot, DocumentUpdate } from "./types";

type PendingDocument = {
  epoch: number;
  path: string;
  revision: number;
  text: string;
  queued: string | null;
  running: Promise<void> | null;
  error: unknown;
};

/** One request per document; further keystrokes replace the pending text. */
export class DocumentSync {
  private documents = new Map<string, PendingDocument>();
  private submit: (update: DocumentUpdate) => Promise<AppSnapshot>;
  private onSnapshot: (snapshot: AppSnapshot) => void;
  private onError: (error: unknown, epoch: number, path: string) => void;

  constructor(
    submit: (update: DocumentUpdate) => Promise<AppSnapshot>,
    onSnapshot: (snapshot: AppSnapshot) => void,
    onError: (error: unknown, epoch: number, path: string) => void
  ) { this.submit = submit; this.onSnapshot = onSnapshot; this.onError = onError; }

  queue(epoch: number, path: string, revision: number, text: string) {
    const key = JSON.stringify([epoch, path]);
    let document = this.documents.get(key);
    if (document === undefined) {
      document = { epoch, path, revision, text, queued: null, running: null, error: null };
      this.documents.set(key, document);
    }
    document.text = text;
    document.queued = text;
    if (document.running === null && document.error === null) this.start(document);
  }

  pendingText(epoch: number, path: string): string | null {
    return this.documents.get(JSON.stringify([epoch, path]))?.text ?? null;
  }

  resolveFailure(epoch: number, path: string, revision: number | null) {
    const key = JSON.stringify([epoch, path]);
    const document = this.documents.get(key);
    if (document === undefined || document.error === null) return;
    if (revision === null) {
      this.documents.delete(key);
      return;
    }
    document.revision = revision;
    document.queued = document.text;
    document.error = null;
    this.start(document);
  }

  async flush() {
    while (this.documents.size > 0) {
      const documents = [...this.documents.values()];
      const failed = documents.find((document) => document.error !== null);
      if (failed !== undefined) throw failed.error;
      await Promise.all(documents.flatMap((document) => document.running === null ? [] : [document.running]));
    }
  }

  private start(document: PendingDocument) {
    document.running = this.drain(document).catch((error: unknown) => {
      document.error = error;
      this.onError(error, document.epoch, document.path);
    });
  }

  private async drain(document: PendingDocument) {
    while (document.queued !== null) {
      const text = this.takeQueued(document);
      const snapshot = await this.submit({
        projectEpoch: document.epoch, path: document.path,
        expectedDocumentRevision: document.revision, text
      });
      const acknowledged = snapshot.tabs.find((buffer) => buffer.path === document.path);
      // The snapshot carries the active document's text while it is shown as text.
      const acknowledgedText = snapshot.activeBuffer?.path === document.path ? snapshot.activeText : null;
      if (snapshot.projectEpoch !== document.epoch || acknowledged === undefined || (acknowledgedText !== null && acknowledgedText !== text)) {
        throw new Error("The text acknowledgement did not match the submitted document.");
      }
      document.revision = acknowledged.documentRevision;
      if (document.queued === text) document.queued = null;
      if (document.queued === null) this.documents.delete(JSON.stringify([document.epoch, document.path]));
      this.onSnapshot(snapshot);
    }
    document.running = null;
  }

  private takeQueued(document: PendingDocument): string {
    const text = document.queued;
    if (text === null) throw new Error("No document text is queued.");
    document.queued = null;
    return text;
  }
}
