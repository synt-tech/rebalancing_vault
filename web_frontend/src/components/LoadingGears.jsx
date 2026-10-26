const BASE = import.meta.env.BASE_URL

export default function LoadingGears({ size = 'md', className = '' }) {
  const config = {
    xxs: { cls: 'w-3 h-3', src: `${BASE}gear7.svg` },
    xs: { cls: 'w-10 h-10', src: `${BASE}gear7.svg` },
    sm: { cls: 'w-16 h-16', src: `${BASE}gears.svg` },
    md: { cls: 'w-20 h-20', src: `${BASE}gears.svg` },
    lg: { cls: 'w-24 h-24', src: `${BASE}gears.svg` },
  }[size] || { cls: size, src: `${BASE}gears.svg` }

  return <img src={config.src} alt="loading" className={`${config.cls} ${className}`} />
}
