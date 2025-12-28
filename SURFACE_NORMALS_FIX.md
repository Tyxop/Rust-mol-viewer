# Fix: Normales Invertidas en Surface Rendering

## 🐛 PROBLEMA

Al rotar el modelo en modo Surface, **la mitad de la superficie se ve oscura/invertida** y la otra mitad se ve correcta. Esto indica que las normales están apuntando en direcciones inconsistentes.

### Síntoma Visual
```
   Rotación 0°         Rotación 180°
   ┌─────────┐        ┌─────────┐
   │  ✓ Bien │        │ ✗ Oscuro│
   │         │   →    │         │
   │ ✗ Oscuro│        │  ✓ Bien │
   └─────────┘        └─────────┘
```

## 🔍 DIAGNÓSTICO

### Código Original (Incorrecto)

**Ubicación:** `crates/mol-render/src/marching_cubes.rs:172-175`

```rust
// Calculate normal using cross product
let edge1 = v1 - v0;
let edge2 = v2 - v0;
let normal = edge1.cross(edge2).normalize();
```

### Problema

**Cross product depende del orden de los vértices:**
- `edge1 × edge2` → normal apunta en una dirección
- `edge2 × edge1` → normal apunta en dirección **opuesta**

En marching cubes, la **TRI_TABLE** define el orden de vértices para cada configuración de cubo. Si este orden no es consistente (algunos triángulos CW, otros CCW), el cross product genera normales que apuntan en direcciones aleatorias.

**Resultado:** Mitad de triángulos con normales hacia afuera, mitad hacia adentro.

---

## ✅ SOLUCIÓN

### Usar Gradiente del SDF

El **gradiente del campo de distancia firmada (SDF)** siempre apunta en la dirección del aumento del campo, que es **perpendicular a la superficie y hacia afuera**.

**Ventajas:**
- ✅ Siempre consistente
- ✅ Independiente del orden de vértices
- ✅ Más preciso que cross product
- ✅ Suaviza automáticamente

### Implementación

**1. Función para calcular gradiente:**

```rust
/// Compute gradient (normal) at a position using central differences
fn compute_gradient(grid: &VoxelGrid, pos: Vec3) -> Vec3 {
    let spacing = grid.spacing;

    // Convert world position to grid coordinates
    let grid_pos = (pos - grid.origin) / spacing;
    let x = grid_pos.x as usize;
    let y = grid_pos.y as usize;
    let z = grid_pos.z as usize;

    let (nx, ny, nz) = grid.dimensions;

    // Central differences with bounds checking
    let dx = if x > 0 && x < nx - 1 {
        (grid.get(x + 1, y, z) - grid.get(x - 1, y, z)) / (2.0 * spacing)
    } else if x == 0 {
        (grid.get(x + 1, y, z) - grid.get(x, y, z)) / spacing
    } else {
        (grid.get(x, y, z) - grid.get(x - 1, y, z)) / spacing
    };

    // Similar for dy, dz...

    let gradient = Vec3::new(dx, dy, dz);
    gradient.normalize()
}
```

**Matemáticas:**
- **Central difference:** `f'(x) ≈ [f(x+h) - f(x-h)] / (2h)`
- **Forward difference (borde):** `f'(x) ≈ [f(x+h) - f(x)] / h`
- **Backward difference (borde):** `f'(x) ≈ [f(x) - f(x-h)] / h`

**2. Uso en marching cubes:**

```rust
// Antes (incorrecto):
let edge1 = v1 - v0;
let edge2 = v2 - v0;
let normal = edge1.cross(edge2).normalize();

vertices.push(Vertex::new(v0, normal));
vertices.push(Vertex::new(v1, normal));
vertices.push(Vertex::new(v2, normal));

// Después (correcto):
let n0 = compute_gradient(grid, v0);
let n1 = compute_gradient(grid, v1);
let n2 = compute_gradient(grid, v2);

vertices.push(Vertex::new(v0, n0));
vertices.push(Vertex::new(v1, n1));
vertices.push(Vertex::new(v2, n2));
```

---

## 📊 COMPARACIÓN

| Método | Consistencia | Precisión | Performance | Suavizado |
|--------|--------------|-----------|-------------|-----------|
| **Cross Product** | ❌ Depende de orden | Media | Rápido | No |
| **Gradiente SDF** | ✅ Siempre correcto | Alta | Medio | ✅ Sí |

### Performance

**Cross product:**
- 2 restas vectoriales + 1 cross product + normalize
- ~10 operaciones

**Gradiente:**
- 6 lecturas de grid + 6 restas + 3 divisiones + normalize
- ~20 operaciones

**Impacto:** ~2x más lento por vértice, pero **mínimo impacto total** porque:
- Generación de superficie es un one-time cost
- Se hace en CPU paralelo con Rayon
- Tiempo dominado por SDF calculation, no normals

---

## 🎨 BENEFICIOS ADICIONALES

### 1. Normales Suavizadas
Con cross product, cada vértice tiene una normal plana del triángulo. Con gradiente, las normales varían suavemente siguiendo el campo SDF.

```
Cross Product:          Gradiente SDF:
   ╱│╲                    ╱│╲
  ╱ │ ╲                  ╱ │ ╲
 ╱  │  ╲                ╱  │  ╲
────┴────              ────┴────
Facetado              Suave
```

### 2. Mejor Iluminación
Normales consistentes → iluminación correcta en toda la superficie.

### 3. Sin Artefactos
No más regiones oscuras al rotar el modelo.

---

## 🧪 TESTING

### Verificar Fix

1. **Ejecutar:**
```bash
cargo run --release --package mol-app -- 9PZW.pdb
```

2. **Cambiar a Surface (tecla 4)**

3. **Rotar modelo 360°:**
   - Click derecho + arrastrar
   - Verificar que toda la superficie se ilumina consistentemente
   - **No debe haber regiones oscuras**

### Casos de Prueba

✅ **Rotación horizontal completa** - Sin cambios de brillo
✅ **Rotación vertical completa** - Sin cambios de brillo
✅ **Zoom in/out** - Iluminación consistente a todas distancias
✅ **Diferentes proteínas** - Funciona con cualquier PDB

---

## 🔧 ARCHIVOS MODIFICADOS

| Archivo | Cambios |
|---------|---------|
| `marching_cubes.rs` | • Añadida función `compute_gradient()`<br>• Reemplazado cross product con gradiente en `process_cube()` |

**Líneas afectadas:** ~70 líneas añadidas, 4 líneas modificadas

---

## 📚 REFERENCIAS TÉCNICAS

### Teoría de Gradientes en SDF

**Propiedad fundamental:**
```
∇SDF(p) ⊥ isosurface en p
```

El gradiente del SDF es **siempre perpendicular** a la isosuperficie (la superficie molecular).

**Dirección:**
- SDF > 0 → fuera de la molécula
- SDF < 0 → dentro de la molécula
- ∇SDF apunta hacia valores **crecientes** → hacia afuera

### Central Differences

**Fórmula estándar:**
```
f'(x) = lim[h→0] (f(x+h) - f(x-h)) / (2h)
```

**En 3D:**
```
∇f = (∂f/∂x, ∂f/∂y, ∂f/∂z)
   ≈ ((f(x+h) - f(x-h))/(2h), ...)
```

**Precisión:** O(h²) vs O(h) de forward/backward differences

---

## 🎯 CONCLUSIÓN

**Problema resuelto:** ✅ Normales ahora apuntan consistentemente hacia afuera

**Mejoras:**
- ✅ Iluminación correcta en toda la superficie
- ✅ Normales suavizadas (mejor calidad visual)
- ✅ Sin artefactos oscuros al rotar
- ✅ Método robusto e independiente del orden de vértices

**Trade-off:**
- ⚠️ ~2x más lento calcular normales (impacto mínimo en total)

El fix es la solución **correcta y estándar** para generar normales en marching cubes. 🚀

---

**Fecha:** 24 Diciembre 2025
**Bug:** Normales invertidas en Surface
**Estado:** ✅ RESUELTO
**Método:** Gradiente SDF con central differences
