// biome-ignore lint/correctness/noUnusedVariables: loaded via importScripts/<script> into global scope
function createLogger(namespace) {
  const prefix = `[vibe-ext:${namespace}]`;
  return {
    debug: (...args) => console.debug(prefix, ...args),
    info: (...args) => console.info(prefix, ...args),
    warn: (...args) => console.warn(prefix, ...args),
    error: (...args) => console.error(prefix, ...args),
  };
}
