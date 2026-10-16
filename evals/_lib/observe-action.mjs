export async function observeAction(observe, action) {
  const observed = observe();
  const [result] = await Promise.all([observed, Promise.resolve().then(action)]);
  return result;
}
