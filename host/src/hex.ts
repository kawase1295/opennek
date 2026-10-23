export const toHex = (b: Uint8Array) => Buffer.from(b).toString('hex')
export const fromHex = (s: string) => {
  if (!/^([0-9a-fA-F]{2})*$/.test(s)) throw new Error(`invalid hex: ${s}`)
  return new Uint8Array(Buffer.from(s, 'hex'))
}
