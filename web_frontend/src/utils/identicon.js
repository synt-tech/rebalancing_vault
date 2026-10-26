const BASE32_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'

function base32Decode(str) {
  const lookup = {}
  BASE32_ALPHABET.split('').forEach((c, i) => { lookup[c] = i })

  const bytes = []
  let buffer = 0
  let bitsLeft = 8

  str.toUpperCase().split('').forEach(char => {
    const val = lookup[char] & 255
    bitsLeft -= 5
    if (bitsLeft > 0) {
      buffer |= val << bitsLeft
    } else if (bitsLeft < 0) {
      bytes.push(buffer | (val >> -bitsLeft))
      bitsLeft += 8
      buffer = (val << bitsLeft) & 255
    } else {
      bytes.push(buffer | val)
      bitsLeft = 8
      buffer = 0
    }
  })

  if (bitsLeft !== 8 && buffer !== 0) {
    bytes.push(buffer)
  }

  return bytes
}

function createMatrix(cols, rows) {
  const matrix = []
  for (let r = 0; r < rows; r++) {
    const row = []
    for (let c = 0; c < cols; c++) row.push(false)
    matrix.push(row)
  }
  return matrix
}

function getBit(index, bytes) {
  return (bytes[Math.floor(index / 8)] & (1 << (7 - (index % 8)))) ? 1 : 0
}

function generateSymmetricPattern(bytes, mirror) {
  const size = 7
  const matrix = createMatrix(size, size)
  const halfCols = mirror ? Math.ceil(size / 2) : size

  for (let col = 0; col < halfCols; col++) {
    for (let row = 0; row < size; row++) {
      if (getBit(col + row * halfCols, bytes.slice(1))) {
        matrix[row][col] = true
        if (mirror) {
          matrix[row][size - col - 1] = true
        }
      }
    }
  }

  return matrix
}

function hsvToRgb(h, s, v) {
  const i = Math.floor(h * 6)
  const f = h * 6 - i
  const p = v * (1 - s)
  const q = v * (1 - f * s)
  const t = v * (1 - (1 - f) * s)

  let r, g, b
  switch (i % 6) {
    case 0: r = v; g = t; b = p; break
    case 1: r = q; g = v; b = p; break
    case 2: r = p; g = v; b = t; break
    case 3: r = p; g = q; b = v; break
    case 4: r = t; g = p; b = v; break
    case 5: r = v; g = p; b = q; break
  }

  return {
    r: Math.round(r * 255),
    g: Math.round(g * 255),
    b: Math.round(b * 255),
  }
}

function drawIdenticon(canvas, publicKey) {
  const decoded = base32Decode(publicKey).slice(2, 16)
  const pattern = generateSymmetricPattern(decoded, true)

  const ctx = canvas.getContext('2d')
  ctx.clearRect(0, 0, canvas.width, canvas.height)

  // Pattern color from first byte (hue), background stays transparent
  const color = hsvToRgb(decoded[0] / 255, 0.7, 0.8)
  ctx.fillStyle = `rgb(${color.r}, ${color.g}, ${color.b})`

  // Draw pattern with padding so the circle doesn't crop edge cells
  const padding = canvas.width * 0.15
  const gridSize = canvas.width - padding * 2
  const cellSize = gridSize / pattern.length
  for (let row = 0; row < pattern.length; row++) {
    for (let col = 0; col < pattern[row].length; col++) {
      if (pattern[row][col]) {
        ctx.fillRect(padding + cellSize * col, padding + cellSize * row, cellSize, cellSize)
      }
    }
  }
}

export function generateIdenticon(publicKey, size = 210) {
  if (!publicKey) return null
  const canvas = document.createElement('canvas')
  canvas.width = size
  canvas.height = size
  drawIdenticon(canvas, publicKey)
  return canvas.toDataURL()
}
