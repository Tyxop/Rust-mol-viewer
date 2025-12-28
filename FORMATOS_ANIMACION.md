# Formatos de Animación Molecular

## 📊 Opciones de Formatos para Trayectorias MD

### 1. **PDB Multi-Model** ⭐ (Recomendado para empezar)

**Estructura:**
```
MODEL 1
ATOM   1  CA  ALA A   1      10.0  20.0  30.0
...
ENDMDL
MODEL 2
ATOM   1  CA  ALA A   1      10.1  20.1  30.1
...
ENDMDL
```

**Pros:**
- ✅ Ya tienes parser PDB implementado
- ✅ Formato texto, fácil debug
- ✅ Compatible con todas las herramientas
- ✅ ~10 líneas de código para soporte

**Contras:**
- ⚠️ Tamaño grande (~1-5 MB/frame para proteína mediana)
- ⚠️ Lento de parsear (texto)
- ⚠️ No apto para trayectorias largas (>1000 frames)

**Tamaño ejemplo:**
- 27K átomos, 1000 frames = **~3 GB** sin comprimir
- Con gzip = **~300 MB**

**Uso típico:** Trayectorias cortas (10-100 frames), visualización rápida

---

### 2. **DCD (CHARMM/NAMD/VMD)** ⭐⭐⭐ (Más común)

**Formato:** Binario, solo coordenadas

**Estructura:**
```
Header (276 bytes):
  - Magic: "CORD"
  - N frames
  - Start timestep
  - Save frequency
  - Box dimensions

Frame data (repite N veces):
  - X coords (4 bytes × N atoms)
  - Y coords (4 bytes × N atoms)
  - Z coords (4 bytes × N atoms)
```

**Pros:**
- ✅ **Muy usado** en MD (NAMD, CHARMM, VMD)
- ✅ **Compacto**: solo 12 bytes/átomo/frame
- ✅ **Rápido**: binario, lectura directa
- ✅ Soporta box dimensions (para PBC)

**Contras:**
- ⚠️ Solo coordenadas (necesitas PDB/PSF para topología)
- ⚠️ Precisión simple (float32, ~0.0001 Å)

**Tamaño ejemplo:**
- 27K átomos, 1000 frames = **~324 MB** (12 bytes × 27K × 1000)
- **10x más pequeño** que PDB multi-model

**Uso típico:** Trayectorias MD largas, estándar de facto

---

### 3. **XTC (Gromacs)** ⭐⭐⭐ (Mejor compresión)

**Formato:** Binario comprimido con algoritmo lossy

**Características:**
- Compresión inteligente (3D grid quantization)
- Precisión configurable (típico: 0.001 nm = 0.01 Å)

**Pros:**
- ✅ **Muy compacto**: ~2-4 bytes/átomo/frame
- ✅ **Rápido**: usado por Gromacs (optimizado)
- ✅ Soporta box dimensions

**Contras:**
- ⚠️ **Compresión lossy** (pérdida de precisión configurable)
- ⚠️ Parser más complejo (algoritmo XDR comprimido)
- ⚠️ Solo coordenadas

**Tamaño ejemplo:**
- 27K átomos, 1000 frames = **~81 MB** (3 bytes × 27K × 1000)
- **40x más pequeño** que PDB multi-model

**Uso típico:** Trayectorias muy largas (>10K frames), Gromacs users

---

### 4. **TRR (Gromacs Full Precision)** ⭐

**Formato:** Binario sin compresión, doble precisión

**Contenido adicional:**
- Coordenadas (double, 8 bytes)
- Velocidades (opcional)
- Fuerzas (opcional)

**Pros:**
- ✅ Máxima precisión (float64)
- ✅ Incluye velocidades/fuerzas

**Contras:**
- ❌ **Muy grande**: ~24+ bytes/átomo/frame
- ❌ Overkill para visualización

**Tamaño ejemplo:**
- 27K átomos, 1000 frames = **~648 MB** (solo coords)

**Uso típico:** Análisis científico preciso, no visualización

---

### 5. **NetCDF/Amber** ⭐⭐

**Formato:** NetCDF4 (HDF5-like), científico estándar

**Pros:**
- ✅ Auto-descriptivo (metadata incluida)
- ✅ Compresión lossless
- ✅ Acceso aleatorio eficiente
- ✅ Multi-plataforma

**Contras:**
- ⚠️ Requiere librería netcdf (dependency)
- ⚠️ Más complejo de parsear

**Tamaño ejemplo:**
- Similar a DCD con compresión (~250 MB)

**Uso típico:** Análisis con Python/MDAnalysis

---

### 6. **XYZ** ⭐⭐ (Más simple)

**Formato:** Texto ASCII simple

**Estructura:**
```
3
Frame 1
C   0.0  0.0  0.0
H   1.0  0.0  0.0
H   0.0  1.0  0.0
3
Frame 2
C   0.1  0.1  0.1
...
```

**Pros:**
- ✅ **Extremadamente simple** de parsear
- ✅ Human-readable
- ✅ Universal

**Contras:**
- ❌ Sin metadata (sin elementos, residuos, etc.)
- ❌ Tamaño grande (texto)
- ❌ No profesional

**Uso típico:** Testing, demos, química cuántica

---

## 🎯 RECOMENDACIÓN DE IMPLEMENTACIÓN

### **Fase 1: Inicio Rápido** (1-2 horas)

**Implementar: PDB Multi-Model**

**Por qué:**
- Ya tienes parser PDB
- Cambio mínimo:
  ```rust
  // Detectar MODEL/ENDMDL
  if line.starts_with("MODEL") {
      current_frame = parse_model_number(line);
      frames.push(Frame::new());
  }
  ```
- Testing inmediato con archivos existentes

**Código estimado:**
```rust
pub struct Trajectory {
    pub topology: Protein,  // Frame 0 o archivo PDB separado
    pub frames: Vec<Frame>,
}

pub struct Frame {
    pub coords: Vec<Vec3>,  // Solo coordenadas (topología compartida)
    pub time: f32,          // Timestamp
}
```

---

### **Fase 2: Producción** (3-5 horas)

**Implementar: DCD (NAMD/CHARMM)**

**Por qué:**
- Formato estándar MD
- ~100 líneas de código
- 10x más eficiente que PDB

**Librerías Rust disponibles:**
- ❌ No hay crate maduro
- ✅ **Implementación manual**: formato simple (ver spec abajo)

**Parser DCD sketch:**
```rust
pub fn parse_dcd(path: &str) -> Result<Trajectory> {
    let mut file = File::open(path)?;

    // Read header (84 bytes)
    let mut header = [0u8; 84];
    file.read_exact(&mut header)?;

    let magic = &header[0..4]; // "CORD"
    let nframes = i32::from_le_bytes([header[8], header[9], header[10], header[11]]);
    let natoms = i32::from_le_bytes([header[72], header[73], header[74], header[75]]);

    // Read frames
    let mut frames = Vec::new();
    for _ in 0..nframes {
        let mut x = vec![0f32; natoms as usize];
        let mut y = vec![0f32; natoms as usize];
        let mut z = vec![0f32; natoms as usize];

        // Read X coords
        file.read_f32_vec(&mut x)?;
        // Read Y coords
        file.read_f32_vec(&mut y)?;
        // Read Z coords
        file.read_f32_vec(&mut z)?;

        // Interleave into Vec3
        let coords: Vec<Vec3> = (0..natoms)
            .map(|i| Vec3::new(x[i], y[i], z[i]))
            .collect();

        frames.push(Frame { coords, time: 0.0 });
    }

    Ok(Trajectory { frames, ... })
}
```

---

### **Fase 3: Avanzado** (5-8 horas)

**Implementar: XTC (Gromacs)**

**Por qué:**
- Mejor compresión
- Usado por Gromacs (muy popular)

**Librerías disponibles:**
- Buscar crate `xdrfile` o `chemfiles-rs`
- O binding a `libxdrfile` (C library)

---

## 📊 COMPARACIÓN DE FORMATOS

| Formato | Tamaño (27K atoms, 1K frames) | Parser | Velocidad | Precisión | Recomendado |
|---------|-------------------------------|--------|-----------|-----------|-------------|
| **PDB Multi** | ~3 GB (300 MB gzip) | ✅ Fácil | ⚠️ Lento | ✅ Alta | ⭐ Fase 1 |
| **DCD** | ~324 MB | ✅ Medium | ✅ Rápido | ✅ Media | ⭐⭐⭐ Fase 2 |
| **XTC** | ~81 MB | ❌ Difícil | ✅ Rápido | ⚠️ Config | ⭐⭐ Fase 3 |
| **TRR** | ~648 MB | ❌ Difícil | ✅ Rápido | ✅✅ Muy alta | ❌ No |
| **NetCDF** | ~250 MB | ❌ Librería | ✅ Rápido | ✅ Alta | ⭐ Futuro |
| **XYZ** | ~2 GB | ✅✅ Trivial | ⚠️ Lento | ✅ Alta | ⚠️ Testing |

---

## 🎮 UI/UX para Animaciones

### Controles Necesarios

```rust
pub struct AnimationState {
    pub playing: bool,
    pub current_frame: usize,
    pub total_frames: usize,
    pub fps: f32,              // Frames per second
    pub loop_mode: LoopMode,   // Once, Loop, PingPong
}

pub enum LoopMode {
    Once,      // Play once and stop
    Loop,      // Repeat from start
    PingPong,  // Forward then backward
}
```

### Panel de Animación

```
┌─────────────────────────────────┐
│ Animation Controls              │
├─────────────────────────────────┤
│ ▶️ Play   ⏸️ Pause   ⏹️ Stop     │
│                                 │
│ Frame: [====•========] 512/1000│
│                                 │
│ Speed: [=====•===] 30 FPS      │
│                                 │
│ Mode: ○ Once ● Loop ○ PingPong │
│                                 │
│ Time: 5.12 ns / 10.00 ns       │
└─────────────────────────────────┘
```

---

## 🚀 PLAN DE IMPLEMENTACIÓN RECOMENDADO

### **Milestone 1: PDB Multi-Model** (2 horas)
```
✅ Detectar MODEL/ENDMDL en parser
✅ Almacenar múltiples frames
✅ Slider para navegar frames
✅ Botón Play/Pause
```

**Testing:** Crear PDB multi-model de prueba con `cat frame*.pdb`

### **Milestone 2: Controles de Animación** (2 horas)
```
✅ Panel UI de animación
✅ Play/Pause/Stop
✅ FPS control
✅ Loop modes
✅ Interpolación entre frames (opcional)
```

### **Milestone 3: DCD Support** (4 horas)
```
✅ Parser DCD header
✅ Parser DCD frames
✅ Cargar topología (PDB) + trayectoria (DCD) separados
✅ Testing con archivos NAMD reales
```

### **Milestone 4: Performance** (2 horas)
```
✅ Caching de frames en GPU
✅ Pre-carga de frames futuros
✅ Límite de memoria (no cargar todo)
✅ Streaming desde disco si es muy grande
```

---

## 📝 EJEMPLO DE USO

```bash
# Opción 1: PDB multi-model
cargo run --release -- trajectory.pdb

# Opción 2: Topología + trayectoria
cargo run --release -- topology.pdb --trajectory trajectory.dcd

# Opción 3: Con parámetros
cargo run --release -- topology.pdb -t trajectory.xtc --fps 30
```

---

## 🔬 ARCHIVOS DE PRUEBA

### Dónde conseguir trayectorias:

1. **RCSB PDB** - NMR ensembles (multi-model)
   - Ejemplo: `1UBQ` tiene 10 modelos NMR
   - URL: https://www.rcsb.org/

2. **MDAnalysis test files**
   - https://www.mdanalysis.org/MDAnalysisData/
   - DCD, XTC, TRR samples

3. **Gromacs tutorial files**
   - http://www.mdtutorials.com/
   - XTC trayectorias de lisozima

4. **Crear propios:**
```bash
# Repetir un PDB con variaciones
for i in {1..100}; do
    echo "MODEL $i" >> traj.pdb
    cat protein.pdb | grep "^ATOM" | awk '{$6+=rand()*0.1; print}' >> traj.pdb
    echo "ENDMDL" >> traj.pdb
done
```

---

## 🎯 RECOMENDACIÓN FINAL

**Empieza con PDB Multi-Model:**
- Mínimo esfuerzo (~10 líneas)
- Testing inmediato
- Suficiente para demos

**Luego DCD si necesitas más:**
- Formato estándar MD
- Buen balance complejidad/beneficio
- ~100 líneas código

**XTC solo si usuarios lo piden:**
- Requiere librería C o parser complejo
- Beneficio marginal vs DCD

---

**Para tu proyecto, yo recomendaría:**

1. **Ahora:** PDB Multi-Model (rápido, funcional)
2. **Después:** DCD (si necesitas MD real)
3. **Futuro:** XTC (si usuarios de Gromacs lo piden)

¿Quieres que implemente el soporte PDB Multi-Model? Son literalmente ~15 minutos. 🚀
