/**
 * What the address bar says about the view, so that a link leads back to
 * it: `?at=cpu/alu/adder8/fa3&engine=gate`.
 */

/**
 * The query string with `name` set to `value`, or without it when `value`
 * is undefined; everything else the link says is kept. Slashes stay
 * readable: a path in a link should look like a path.
 */
export function searchWith(search: string, name: string, value: string | undefined): string {
  const params = new URLSearchParams(search);
  if (value === undefined) params.delete(name);
  else params.set(name, value);
  const text = params.toString().replaceAll('%2F', '/');
  return text === '' ? '' : `?${text}`;
}
