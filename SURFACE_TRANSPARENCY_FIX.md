# Fix: Artefactos de Transparencia en Surface Rendering

## 🐛 PROBLEMA REPORTADO

**Síntoma Visual:**
- Al rotar 90°, una mitad de la superficie es **semi-transparente** y la otra **opaca**
- En la intersección aparecen **bandas verticales**
- Es un efecto **dinámico** que se mueve al rotar
- Ambos lados tienen iluminación correcta (normales OK)

```
Vista desde un ángulo:
┌─────────────┐
│   Opaco     │ ← Primera mitad
├─────────────┤ ← Bandas verticales (artefacto)
│Transparente │ ← Segunda mitad
└─────────────┘
```

## 🔍 DIAGNÓSTICO

### Problema: Alpha Blending + Depth Write

**Configuración original (INCORRECTA):**

```rust
// surface.rs línea 86
let alpha = 0.7f32;  // Semi-transparente

// línea 140
blend: Some(wgpu::BlendState::ALPHA_BLENDING),  // Alpha blending ON

// línea 156
depth_write_enabled: true,  // ❌ PROBLEMA AQUÍ
```

### Por qué falla:

Cuando renderizas geometría transparente con **depth write enabled**:

1. **GPU dibuja triángulos en orden arbitrario** (no ordenados por profundidad)
2. **Triángulo A** se dibuja primero:
   - Se renderiza con alpha 0.7 (semi-transparente)
   - **Escribe al depth buffer** su valor Z
3. **Triángulo B** (detrás de A) se dibuja después:
   - Depth test compara con Z de A
   - **Falla depth test** → no se dibuja
   - Región queda **opaca** (solo se ve A)
4. **Triángulo C** (delante de A) se dibuja:
   - Pasa depth test
   - Se mezcla con A usando alpha blending
   - Región queda **semi-transparente**

**Resultado:** Caos de opacidad dependiendo del orden de dibujado.

### Visualización del Problema

```
Sin ordenamiento:        Con ordenamiento correcto:
┌─────┬─────┐           ┌─────────────┐
│ ▓▓▓ │ ░░░ │           │   ░░░░░░░   │
│Opaco│Trans│           │ Transparente│
└─────┴─────┘           └─────────────┘
Artefactos              Consistente
```

---

## ✅ SOLUCIÓN IMPLEMENTADA

### Opción A: Superficie Opaca (Implementada)

**Cambios:**

```rust
// 1. Alpha = 1.0 (completamente opaco)
let alpha = 1.0f32; // Fully opaque - fixes transparency artifacts

// 2. Sin alpha blending
blend: None, // No blending needed for opaque surface

// 3. Depth write sigue enabled (correcto para opacos)
depth_write_enabled: true,  // ✅ OK para superficies opacas
```

**Ventajas:**
- ✅ **Sin artefactos** - comportamiento consistente
- ✅ **Mejor performance** - no hay blending
- ✅ **Depth occlusion correcto** - culling de geometría trasera
- ✅ **Solución simple** - 2 líneas cambiadas

**Desventajas:**
- ⚠️ No puedes ver "dentro" de la molécula (superficie opaca)

---

## 🎨 Opción B: Transparencia Correcta (No Implementada)

Si necesitas transparencia real, requiere:

### 1. Deshabilitar Depth Write

```rust
depth_write_enabled: false,  // No escribir al depth buffer
```

### 2. Ordenar Geometría Back-to-Front

```rust
// En render():
// 1. Calcular distancia de cada triángulo a cámara
let triangles_with_depth: Vec<(Triangle, f32)> = triangles
    .iter()
    .map(|tri| {
        let center = (tri.v0 + tri.v1 + tri.v2) / 3.0;
        let depth = (center - camera.position).length();
        (tri.clone(), depth)
    })
    .collect();

// 2. Ordenar de más lejano a más cercano
triangles_with_depth.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

// 3. Dibujar en orden
for (tri, _) in triangles_with_depth {
    draw_triangle(tri);
}
```

### 3. Renderizar en Múltiples Pasadas

```rust
// Pass 1: Objetos opacos (con depth write)
render_opaque_objects();

// Pass 2: Objetos transparentes (sin depth write, ordenados)
render_transparent_objects_sorted();
```

**Complejidad:**
- 🔴 Ordenamiento por CPU en cada frame (~1-2ms para 100K triángulos)
- 🔴 Pérdida de instanced rendering
- 🔴 Necesita restructurar render pipeline

**Por eso NO se implementó** - demasiado complejo para beneficio marginal.

---

## 📊 COMPARACIÓN DE SOLUCIONES

| Aspecto | Opaco (Implementado) | Transparente Correcto | Transparente Bugueado (Original) |
|---------|---------------------|----------------------|-----------------------------------|
| Artefactos | ✅ Ninguno | ✅ Ninguno | ❌ Bandas, inconsistencia |
| Performance | ✅ Óptima | ⚠️ ~2ms overhead | ✅ Óptima |
| Complejidad | ✅ Trivial | 🔴 Alta | ✅ Trivial |
| Ver interior | ❌ No | ✅ Sí | ⚠️ Sí pero bugueado |
| Uso común | ✅ Estándar | ⚠️ Casos específicos | ❌ Nunca |

---

## 🧪 TESTING

### Verificar el Fix

1. **Ejecutar:**
```bash
cargo run --release --package mol-app -- 9PZW.pdb
```

2. **Cambiar a Surface (tecla 4)**
   - Esperar generación (~10-20s)

3. **Rotar 360° en todas direcciones:**
   - Click derecho + arrastrar

4. **Verificar:**
   - ✅ **Superficie completamente opaca** en todos los ángulos
   - ✅ **Sin bandas verticales**
   - ✅ **Sin cambios de opacidad** al rotar
   - ✅ **Color consistente** (gris azulado)

### Antes vs Después

**ANTES (Bug):**
```
Rotación 0°:           Rotación 90°:
┌──────────┐          ┌──────────┐
│  Opaco   │          │Trans│Opa │
│          │    →     │paren│co  │
│          │          │te   │    │
└──────────┘          └─────┴────┘
                      Bandas visibles
```

**DESPUÉS (Fix):**
```
Rotación 0°:           Rotación 90°:
┌──────────┐          ┌──────────┐
│  Opaco   │          │  Opaco   │
│          │    →     │          │
│          │          │          │
└──────────┘          └──────────┘
Consistente           Consistente
```

---

## 🔧 ARCHIVOS MODIFICADOS

| Archivo | Cambios |
|---------|---------|
| `surface.rs:86` | `alpha = 0.7` → `alpha = 1.0` |
| `surface.rs:140` | `blend: Some(ALPHA_BLENDING)` → `blend: None` |

**Total:** 2 líneas modificadas

---

## 📚 REFERENCIAS TÉCNICAS

### Reglas de Transparencia en Graphics

**Regla 1: Depth Write + Alpha Blending = ❌**
```
Si alpha < 1.0:
    depth_write_enabled = false
```

**Regla 2: Transparencias Requieren Ordenamiento**
```
Transparencias deben dibujarse:
- De atrás hacia adelante (back-to-front)
- Después de objetos opacos
- Con depth write disabled
```

**Regla 3: Evitar Transparencias Si Es Posible**
```
Superficies sólidas → alpha = 1.0 (más simple, más rápido)
Efectos especiales → alpha < 1.0 (solo cuando necesario)
```

### Order-Independent Transparency (OIT)

Técnicas avanzadas para transparencia sin ordenamiento:
- **Depth Peeling** - múltiples pasadas
- **A-Buffer** - lista enlazada por píxel
- **Weighted Blended** - aproximación ponderada

**No implementadas** - complejidad muy alta, beneficio bajo para este caso.

---

## 🎯 CONCLUSIÓN

**Problema:** Artefactos de transparencia por depth write enabled con alpha blending

**Solución:** Superficie completamente opaca (`alpha = 1.0`, `blend = None`)

**Resultado:**
- ✅ Sin artefactos visuales
- ✅ Renderizado consistente
- ✅ Mejor performance
- ✅ Solución simple y robusta

**Trade-off aceptable:** Pérdida de transparencia a cambio de calidad visual y simplicidad.

---

## 💡 Mejora Futura (Opcional)

Si en el futuro necesitas transparencia real:

1. Añadir **configuración de alpha en UI**:
```rust
ui.add(Slider::new(&mut alpha, 0.0..=1.0).text("Surface Alpha"));
```

2. Implementar **two-pass rendering**:
```rust
if alpha < 1.0 {
    render_surface_transparent_sorted();
} else {
    render_surface_opaque();  // Current implementation
}
```

3. O usar **weighted blended OIT** (más complejo pero sin ordenamiento).

Por ahora, **superficie opaca es la mejor solución**. 🚀

---

**Fecha:** 24 Diciembre 2025
**Bug:** Artefactos de transparencia en Surface
**Estado:** ✅ RESUELTO
**Método:** Superficie opaca (alpha = 1.0, no blending)
