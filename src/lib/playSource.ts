// Which source an episode plays from. Pure on purpose: scripts/play-source-test.mjs runs it in node.

/** Provider keys that mean "no particular server" (Auto) or "the user's own files". */
const FILE_FRIENDLY = new Set(['auto', 'local']);

/**
 * A downloaded or linked episode plays from its file unless the user picked another server for
 * it. A remembered or preferred server is only a default, so it never beats the file.
 */
export function playsFromFile(input: {
  hasLocalFile: boolean;
  explicitProvider: string | null;
  rememberedProvider?: string | null;
  preferredProvider?: string | null;
}): boolean {
  const { hasLocalFile, explicitProvider } = input;
  return hasLocalFile && (!explicitProvider || FILE_FRIENDLY.has(explicitProvider));
}

/** Extension episode lookups wait until an episode played from a file has started. */
export function episodeLookupsEnabled(input: { fromFile: boolean; playbackStarted: boolean }): boolean {
  return !input.fromFile || input.playbackStarted;
}
