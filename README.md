# Turbopump

Turbopump es una aplicación libre y multiplataforma (Windows, macOS y Linux), escrita en
Rust, para el diseño preliminar e interactivo de turbomáquinas: bombas, ventiladores,
compresores y turbinas.

> **Estado:** fases 0 a 5 y 7 hechas; la 6 (el resto de máquinas) queda pendiente.
> Turbopump diseña una bomba centrífuga completa, con rodete, difusor con o sin álabes y
> voluta, desde el punto de diseño hasta los sólidos CAD (STEP, IGES, STL). Da además la
> curva característica prevista, el flujo potencial meridional y las velocidades en las
> caras del álabe, con interfaz gráfica y línea de comandos. La hoja de ruta completa está
> en [PLAN.md](PLAN.md).

![Editor de Turbopump](docs/img/editor.jpg)

## Qué hay ahora

| Crate | Contenido |
|---|---|
| [`tp-units`](crates/tp-units) | Magnitudes físicas, unidades SI/US, conversión y lectura de texto (`"2900 rpm"`), y velocidad específica (nq, ns US, ωs, σ de Cordier) |
| [`tp-math`](crates/tp-math) | Curvas Bézier y B-spline (evaluación, derivadas, subdivisión, interpolación), polilíneas, interpolación 1D monótona, raíces, cuadratura y sistemas dispersos con gradiente conjugado |
| [`tp-fluids`](crates/tp-fluids) | Propiedades de líquidos con CoolProp, cargado al ejecutar, y un modelo propio del agua (IAPWS) para cuando falta |
| [`tp-design`](crates/tp-design) | Triángulos de velocidad, deslizamiento (Wiesner), correlaciones de bomba, dimensiones principales y geometría del rodete (contorno meridional, flujo meridional, álabes y espesores), voluta y difusores con o sin álabes, curva característica, flujo potencial meridional y velocidades en las caras del álabe |
| [`tp-io`](crates/tp-io) | Exportación: lista de puntos, malla STL y dibujos SVG |
| [`tp-cad`](crates/tp-cad) | Sólidos del rodete, los difusores, la voluta y sus dominios fluidos con Open CASCADE, y exportación STEP, IGES y STL |
| [`tp-project`](crates/tp-project) | Modelo de proyecto, parámetros automáticos o manuales, componentes y sus reglas, pasos de diseño, archivo `.tpj` y deshacer/rehacer |
| [`tp-cli`](crates/tp-cli) | Ejecutable `turbopump` para la línea de comandos |
| [`tp-gui`](crates/tp-gui) | Ejecutable `turbopump-gui`: editor gráfico con egui y vista 3D con wgpu |

## Compilar y probar

Hace falta:

- Rust 1.95 o posterior;
- **cmake**;
- un **compilador C++17** (Xcode Command Line Tools, GCC/Clang o Visual Studio).

La primera compilación construye Open CASCADE 7.8.1 desde el código fuente y tarda de 5 a
15 minutos; las siguientes lo reutilizan. Para compilar sin CAD:

```bash
cargo build -p tp-cli --no-default-features
```

```bash
cargo build -p tp-gui --no-default-features
```

```bash
cargo build --release
```

```bash
cargo test --workspace
```

## Interfaz gráfica

```bash
cargo run --release -p tp-gui
```

También abre un proyecto existente:

```bash
cargo run --release -p tp-gui -- bomba.tpj
```

Arranca con una bomba de 100 m³/h, 32 m y 2900 rpm, con un rodete y una voluta, en tema
claro (también hay tema oscuro). El trabajo sigue los pasos de diseño de cada componente:

- **Barra de herramientas** (arriba), por pestañas:
  - *Home*: proyecto, deshacer, configuración global, unidades, añadir componentes y
    exportar;
  - la pestaña del componente seleccionado, con sus pasos de diseño, el estado de cada
    uno y los botones para aceptar o restablecer el paso;
  - *View*: vistas, cámara, modo de dibujo, paneles (árbol, propiedades y mensajes),
    tema e idioma;
  - *Help*.

  El menú *File* abre, guarda y exporta.
- **Columna de componentes** (izquierda): la configuración global y un pictograma por
  componente, con un punto verde si está terminado o naranja si quedó desactualizado.
  El botón + añade un rodete, un difusor con o sin álabes o una voluta, simple o doble
  (con nervio de nariz redonda o elíptica, que reduce el empuje radial). La voluta
  admite descarga radial (por defecto, central sobre la línea del eje de la bomba) o
  tangencial, abierta a ambos lados o con una pared recta. Por defecto gira para que
  la descarga quede vertical. La lengüeta puede ser tangente, afilada o redondeada con su
  radio.
- **Panel izquierdo**, que se pliega con un botón para dejar más sitio a la vista, con
  dos pestañas:
  - *Design*: los pasos de cada componente, con su estado (pendiente, hecho o
    desactualizado porque cambió un dato anterior) y sus avisos;
  - *3D model*: muestra u oculta cada pieza de la vista 3D o la vuelve translúcida.
- **Vista principal**, una a la vez y a pantalla completa, elegida en la barra de
  encima:
  - vista meridional, donde los puntos de control de Bézier, el borde de ataque y la
    longitud axial se arrastran con el ratón, con las paredes del difusor y las
    secciones de la voluta;
  - gráficas del álabe (β, θ y espesor);
  - vista frontal de la etapa, con los álabes del difusor y la espiral;
  - vista 3D, con aristas de las piezas, secciones de la voluta, cubo de vistas (un clic
    en una cara gira la cámara), ejes y barra de escala;
  - curvas: altura, rendimiento, potencia y NPSH₃ previstos frente al caudal, con la
    zona de recirculación estimada a trazos, el NPSH disponible y la pérdida en la
    voluta, que entra en la curva de altura; también los empujes radial y axial sobre
    el rodete, con o sin agujeros de equilibrado, el rendimiento por suma de pérdidas
    (fugas por los anillos, rozamiento de disco, pérdidas mecánicas y pérdidas
    hidráulicas del rodete y la voluta), la corrección
    para líquidos viscosos y el punto de trabajo en la instalación, con otra velocidad
    o el rodete recortado y el rango de funcionamiento continuo;
  - informe con las mismas cifras que la línea de comandos.

  La vista meridional muestra también, si se pide, el flujo potencial y la velocidad
  meridional en cubo y carcasa. La del álabe añade la velocidad relativa en sus dos
  caras.

  Al abrir un paso, la vista cambia a la que mejor lo muestra (se puede desactivar en
  la pestaña *View*).
- **Panel de propiedades** (derecha): al elegir un paso o la configuración global se
  abre con sus datos. Cada valor es automático (propuesto por una correlación) o
  manual. *Accept step* marca el paso como hecho y pasa al siguiente, y los errores y
  avisos salen en tarjetas. El panel flota sobre la vista sin reducirla: se pliega a una
  franja estrecha (con su flecha, con Esc o con el botón *Properties*), se ensancha
  arrastrando su borde izquierdo y no bloquea las vistas, así que con él abierto se
  siguen arrastrando los puntos del contorno. La vista 3D centra la pieza en la parte
  que queda libre.
- **Barra de estado** (abajo): el último mensaje, las unidades y el contador de errores
  y avisos, que abre el panel de mensajes de todo el proyecto.
- **Idioma:** inglés o español, en la pestaña *View*. La elección se guarda, y la
  primera vez se toma la del sistema. Los avisos del diseño también se traducen.
- **Edición:** todo se puede deshacer y rehacer (Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z). Un
  arrastre completo cuenta como un solo paso.
- **Exportación:** puntos, malla STL, dibujos SVG y sólidos CAD STEP, IGES o STL, que se
  construyen en segundo plano.

Las pruebas de la interfaz usan [kittest](https://github.com/rerun-io/kittest) y no
necesitan GPU. Las capturas con wgpu se generan a mano en `target/screenshots/`:

```bash
cargo test -p tp-gui --test screenshots -- --ignored
```

## Ejemplos de uso

Velocidad específica de una bomba de 100 m³/h, 32 m de altura y 2900 rpm:

```bash
cargo run -p tp-cli -- nq --flow "100 m3/h" --head "32 m" --speed "2900 rpm"
```

Crear un proyecto nuevo y ver su resumen:

```bash
cargo run -p tp-cli -- new bomba.tpj --machine pump --flow "100 m3/h" --head "32 m" --speed "2900 rpm"
```

```bash
cargo run -p tp-cli -- info bomba.tpj
```

Calcular las dimensiones principales del rodete. El resultado se guarda en el proyecto, y
cualquier valor se puede fijar con `--set` (los números sin unidad usan las del proyecto):

```bash
cargo run -p tp-cli -- design bomba.tpj
```

```bash
cargo run -p tp-cli -- design bomba.tpj --set d2=180 --set z=6
```

Construir la geometría del rodete (contorno meridional, flujo meridional, álabes,
espesores y bordes) y exportar puntos, malla STL y dibujos:

```bash
cargo run -p tp-cli -- geometry bomba.tpj --export salida
```

| Vista meridional | Vista frontal |
|---|---|
| ![meridional](docs/img/rodete-referencia-meridional.svg) | ![frontal](docs/img/rodete-referencia-frontal.svg) |

Sólidos CAD del rodete y del dominio fluido (Open CASCADE):

```bash
cargo run -p tp-cli -- cad bomba.tpj --format step,iges,stl
```

| Rodete | Dominio fluido |
|---|---|
| ![rodete](docs/img/rodete-referencia-cad.jpg) | ![fluido](docs/img/fluido-referencia-cad.jpg) |

Una bomba completa se crea con sus componentes de la entrada a la salida: rodete,
estator con álabes (`stator`) o sin ellos (`vaneless-stator`) y voluta. Cada uno se
diseña con su orden, y `cad` construye los sólidos de todos:

```bash
cargo run -p tp-cli -- new etapa.tpj --machine pump --flow "100 m3/h" --head "32 m" --speed "2900 rpm" --components impeller,stator,volute
```

```bash
cargo run -p tp-cli -- design etapa.tpj
```

```bash
cargo run -p tp-cli -- geometry etapa.tpj
```

```bash
cargo run -p tp-cli -- diffuser etapa.tpj
```

```bash
cargo run -p tp-cli -- volute etapa.tpj --set shape=circle --export salida
```

```bash
cargo run -p tp-cli -- cad etapa.tpj --format step
```

La teoría y las cifras de la bomba de referencia están en
[docs/theory/voluta-y-difusores.md](docs/theory/voluta-y-difusores.md).

Curva característica prevista, de válvula cerrada a sobrecarga, también en CSV:

```bash
cargo run -p tp-cli -- performance bomba.tpj --csv curva.csv
```

Flujo potencial meridional y velocidades en las caras del álabe:

```bash
cargo run -p tp-cli -- analysis bomba.tpj
```

| Curva característica | Flujo potencial |
|---|---|
| ![curvas](docs/img/curvas-referencia.jpg) | ![flujo potencial](docs/img/flujo-potencial.jpg) |

El método y sus límites están en [docs/theory/prestaciones.md](docs/theory/prestaciones.md).

Propiedades de un líquido:

```bash
cargo run -p tp-cli -- fluid Water --temperature "60 °C"
```

Convertir unidades:

```bash
cargo run -p tp-cli -- convert 150 psi bar
```

## CoolProp

Turbopump usa [CoolProp](http://www.coolprop.org) para las propiedades de los fluidos y lo
carga al ejecutarse. Busca `libCoolProp.dylib` (macOS), `libCoolProp.so` (Linux) o
`CoolProp.dll` (Windows) en este orden:

1. la ruta de la variable `TURBOPUMP_COOLPROP`;
2. junto al ejecutable;
3. en el sistema.

Si no la encuentra, el agua sigue disponible con un modelo propio basado en IAPWS.
`TURBOPUMP_COOLPROP=none` desactiva CoolProp a propósito.

Durante el desarrollo se puede usar la copia que descarga `cargo` para las pruebas:

```bash
export TURBOPUMP_COOLPROP="$(cargo run -q -p tp-fluids --example coolprop_path)"
```

## Licencia

Puedes usar el proyecto bajo cualquiera de estas dos licencias, a tu elección:

- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT ([LICENSE-MIT](LICENSE-MIT))

Los binarios con CAD incluyen Open CASCADE enlazado de forma estática (LGPL-2.1 con la
excepción de Open CASCADE). El código fuente completo está disponible para volver a
enlazarlo.

La interfaz gráfica incluye la fuente Noto Sans, bajo la SIL Open Font License 1.1
([texto](crates/tp-gui/LICENSE-OFL-NotoSans.txt)), y los iconos
[Phosphor](https://phosphoricons.com), bajo licencia MIT.

Las contribuciones se aceptan bajo esas mismas dos licencias. Antes de contribuir, lee
[CONTRIBUTING.md](CONTRIBUTING.md), en especial las reglas de sala limpia.
