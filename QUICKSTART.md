# Inicio Rápido - PDB Visual

## 1. Instalar Rust

Si no tienes Rust instalado:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

Verifica la instalación:
```bash
cargo --version
```

## 2. Compilar el Proyecto

```bash
# Compilación en modo release (optimizada)
cargo build --release
```

Esto descargará todas las dependencias y compilará el proyecto. La primera vez puede tardar varios minutos.

## 3. Ejecutar con el Archivo PDB de Ejemplo

```bash
cargo run --release --package mol-app -- 9PZW.pdb
```

Deberías ver:
- Una ventana de 1280×720 con el título "PDB Visual - Molecular Viewer"
- Una esfera naranja en el centro (esfera de prueba)
- En la consola: información sobre el archivo PDB cargado

## 4. Controles

- **Clic izquierdo + arrastrar**: Rotar la cámara alrededor del modelo
- **Clic derecho + arrastrar**: Mover la cámara lateralmente
- **Rueda del ratón**: Acercar/alejar zoom
- **ESC**: Cerrar la aplicación

## 5. Logs y Debug

Para ver más información de debug:

```bash
RUST_LOG=debug cargo run --release --package mol-app -- 9PZW.pdb
```

Niveles de log disponibles:
- `error`: Solo errores
- `warn`: Advertencias y errores
- `info`: Información general (default)
- `debug`: Información detallada
- `trace`: Todo

## Próximos Pasos

La **Fase 1** está completada. Para continuar con el desarrollo:

### Fase 2: Representaciones Visuales
1. Implementar renderizado de esferas VdW con tamaños atómicos reales
2. Añadir ball-and-stick con cilindros para enlaces
3. Crear ribbon/cartoon con splines
4. Implementar superficie molecular

### Fase 3: Optimizaciones
1. Sistema LOD automático basado en distancia a cámara
2. Frustum culling con octree espacial
3. Optimizar instance buffers

### Fase 4: VR
1. Integrar OpenXR
2. Renderizado estéreo
3. Input de controllers

### Fase 5: Features Avanzadas
1. Compute shaders para superficies
2. UI con egui
3. Múltiples esquemas de color

## Solución de Problemas

### Error: "command not found: cargo"
Asegúrate de haber instalado Rust y ejecutado `source $HOME/.cargo/env`

### Error al compilar wgpu
En Linux, instala las dependencias del sistema:
```bash
# Ubuntu/Debian
sudo apt install libwayland-dev libxkbcommon-dev

# Arch
sudo pacman -S wayland libxkbcommon
```

### La ventana no se abre
Verifica que tu GPU soporte Vulkan (Linux/Windows) o Metal (macOS).

### Rendimiento lento
Asegúrate de compilar en modo `--release` para optimizaciones completas.

## Información del Sistema

Para verificar que wgpu detecta tu GPU correctamente, el log debería mostrar:
```
INFO  Renderer initialized successfully
INFO    Surface format: ...
INFO    Size: 1280x720
INFO    Icosphere: ... vertices, ... indices
```

Si ves errores sobre "No adapter found", tu GPU podría no soportar las APIs gráficas necesarias.
