import { useMemo } from 'react'
import { generateIdenticon } from '../utils/identicon'

export default function Identicon({ publicKey, size = 24, className = '', alt = 'account identicon' }) {
  const dataUrl = useMemo(() => {
    if (!publicKey) return null
    try {
      return generateIdenticon(publicKey, Math.max(size * 4, 128))
    } catch {
      return null
    }
  }, [publicKey, size])

  if (!dataUrl) return null

  return (
    <img
      src={dataUrl}
      alt={alt}
      width={size}
      height={size}
      className={`rounded-full bg-synt_tech_darker ${className}`}
      style={{ imageRendering: 'pixelated' }}
    />
  )
}
