import type { AppSnapshot, DocumentTextEdit, DocumentUpdate } from "./types";

type PendingDocument = {
  epoch: number;
  path: string;
  /** The backend revision the queued edits apply to. */
  revision: number;
  /** Batches of edits, in order, not yet sent. */
  queued: DocumentTextEdit[][];
  /** The editor's current text, to send whole when its edits cannot apply. */
  text: () => string;
  running: Promise<void> | null;
  error: unknown;
};

/** A range past the last line ends at the text's end. */
const WHOLE_TEXT = { start: { line: 0, character: 0 }, end: { line: 0xffffffff, character: 0 } };

/**
 * Editors' edits to backend documents. One request per document is in
 * flight; edits made meanwhile are sent together in the next. Each editor's
 * text is a known backend revision plus its queued edits.
 */
export class DocumentSync {
  private documents = new Map<string, PendingDocument>();
  /** The backend revision each editor's text is, apart from queued edits. */
  private revisions = new Map<string, number>();
  private submit: (update: DocumentUpdate) => Promise<AppSnapshot>;
  private onSnapshot: (snapshot: AppSnapshot) => void;
  private onError: (error: unknown, epoch: number, path: string) => void;
  private onPending: (pending: boolean) => void;

  constructor(
    submit: (update: DocumentUpdate) => Promise<AppSnapshot>,
    onSnapshot: (snapshot: AppSnapshot) => void,
    onError: (error: unknown, epoch: number, path: string) => void,
    onPending: (pending: boolean) => void
  ) { this.submit = submit; this.onSnapshot = onSnapshot; this.onError = onError; this.onPending = onPending; }

  /** An editor holds the document's text at `revision`. */
  loaded(epoch: number, path: string, revision: number) {
    this.revisions.set(key(epoch, path), revision);
  }

  /** The backend revision an editor's text is, or null before it loads. */
  revision(epoch: number, path: string): number | null {
    return this.revisions.get(key(epoch, path)) ?? null;
  }

  /** Whether an editor has edits the backend has not acknowledged. */
  pending(epoch: number, path: string): boolean {
    return this.documents.has(key(epoch, path));
  }

  queue(epoch: number, path: string, edits: DocumentTextEdit[], text: () => string) {
    const revision = this.revisions.get(key(epoch, path));
    if (revision === undefined) throw new Error("The document was edited before its text loaded.");
    let document = this.documents.get(key(epoch, path));
    if (document === undefined) {
      document = { epoch, path, revision, queued: [], text, running: null, error: null };
      this.documents.set(key(epoch, path), document);
      this.onPending(true);
    }
    document.queued.push(edits);
    if (document.running === null && document.error === null) this.start(document);
  }

  /**
   * Settle a failed document: with the backend's current revision, send the
   * editor's whole text over it; with null, drop the edits so the editor
   * reloads the backend's text.
   */
  resolveFailure(epoch: number, path: string, revision: number | null) {
    const document = this.documents.get(key(epoch, path));
    if (document === undefined || document.error === null) return;
    if (revision === null) {
      this.documents.delete(key(epoch, path));
      this.revisions.set(key(epoch, path), -1);
      this.onPending(this.documents.size > 0);
      return;
    }
    document.revision = revision;
    document.queued = [[{ range: WHOLE_TEXT, text: document.text() }]];
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
    while (document.queued.length > 0) {
      const changes = document.queued;
      document.queued = [];
      const snapshot = await this.submit({
        projectEpoch: document.epoch, path: document.path,
        expectedDocumentRevision: document.revision, changes
      });
      const acknowledged = snapshot.tabs.find((buffer) => buffer.path === document.path);
      if (snapshot.projectEpoch !== document.epoch || acknowledged === undefined) {
        throw new Error("The edit acknowledgement did not match the submitted document.");
      }
      document.revision = acknowledged.documentRevision;
      this.revisions.set(key(document.epoch, document.path), document.revision);
      if (document.queued.length === 0) {
        this.documents.delete(key(document.epoch, document.path));
        this.onPending(this.documents.size > 0);
      }
      this.onSnapshot(snapshot);
    }
    document.running = null;
  }
}

function key(epoch: number, path: string): string {
  return JSON.stringify([epoch, path]);
}
