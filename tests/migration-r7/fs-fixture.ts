export const counters = { read: 0, write: 0, stat: 0, largeRead: 0, model: 0 };
export async function stat(path: string) {
  counters.stat++;
  return { isFile: true, size: path.includes("large") ? 564 * 1024 * 1024 : 8 };
}
export async function readTextFile(path: string) {
  counters.read++;
  if (path.includes("large")) {
    counters.largeRead++;
    throw new Error("Dangerous read reached");
  }
  return localStorage.getItem("r7-fixture-content") ?? "hello 小雨";
}
export async function writeTextFile(_path: string, content: string) {
  counters.write++;
  localStorage.setItem("r7-fixture-content", content);
}
export async function remove() {}
export async function mkdir() {}

export async function exists() {
  return false;
}
export async function readDir() {
  return [];
}
