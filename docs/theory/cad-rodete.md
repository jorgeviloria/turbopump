# Sólidos CAD del rodete

Implementación: [`crates/tp-cad/`](../../crates/tp-cad/)

| Rodete (STL exportado) | Dominio fluido |
|---|---|
| ![rodete](../img/rodete-referencia-cad.jpg) | ![fluido](../img/fluido-referencia-cad.jpg) |

*Rodete de referencia: 100 m³/h, 32 m, 2900 rpm, todos los valores automáticos.*

## Núcleo CAD

Se usa **Open CASCADE (OCCT) 7.8.1**, el núcleo CAD libre de referencia. El crate `occt-sys` lo
compila desde el código fuente como librerías estáticas: no hace falta instalar nada
aparte de **cmake** y un **compilador C++17**. La primera compilación tarda de 5 a 15
minutos; después se reutiliza.

Rust no llama a OCCT directamente, sino a través de un **puente C++ pequeño y propio**
([`cpp/tp_cad.cpp`](../../crates/tp-cad/cpp/tp_cad.cpp), con `cxx`):

- **Excepciones.** Cada llamada a OCCT va dentro de un `try/catch`. Las excepciones de
  OCCT (`Standard_Failure`, que no deriva de `std::exception`) llegan a Rust como
  `Err`, en lugar de abortar el proceso.
- **Clases que faltan en los enlaces publicados.** El puente usa directamente lo que
  `opencascade-sys` no expone: cosido, relleno de caras, reparación y validación.
- **Orden de destrucción.** En un enlace estático, los objetos globales de STEP/IGES
  podían sobrevivir al registro de tipos de OCCT y el programa fallaba al salir. El
  puente crea ese registro en su propia inicialización estática, antes que cualquier
  otro objeto de OCCT.

Todo el modelo se construye en **milímetros**, que es la unidad por defecto de los
escritores STEP e IGES.

## Construcción

1. **Canal.** El perfil meridional (cubo, salida, carcasa y entrada) se interpola con
   B-splines en el plano xz y se revoluciona 360° alrededor de z.
2. **Álabe.** Es un *loft* (`BRepOffsetAPI_ThruSections`) por las secciones de la
   geometría, de modo que sus caras laterales son superficies B-spline.
   - **Cada sección** es un contorno cerrado: cara de succión, nariz elíptica, cara de
     presión y salida cortada.
   - **Prolongación entre cubo y carcasa.** Se añade una sección extrapolada fuera del
     cubo y otra fuera de la carcasa, a 0,5 veces el paso entre secciones, para que las
     booleanas no trabajen con caras coincidentes.
   - **Prolongación de la salida.** La salida cortada se prolonga más allá de d₂. Si no,
     la cuerda entre las dos caras dejaría una lente de fluido de 8,6 mm³ por álabe
     pegada a la salida, separada del resto.
   - **Cierre.** Los extremos no son planos. Se cierran con caras de relleno
     (`BRepOffsetAPI_MakeFilling`) sobre los bordes libres de la cáscara; después se
     cose, se convierte en sólido, se repara (`ShapeFix_Solid`) y se valida
     (`BRepCheck_Analyzer`).
3. **Dominio fluido** = canal − álabes (z copias giradas).
4. **Rodete** = disco del cubo ∪ carcasa ∪ (álabes ∩ canal).
   - **Disco del cubo:** macizo desde el contorno del cubo hasta el diámetro del eje, con
     una cara trasera a un espesor de pared detrás de la salida.
   - **Carcasa:** el contorno de la carcasa con espesor hacia fuera.
   - **Espesor de pared por defecto:** max(3 mm; 0,02·d₂). Es un valor por defecto de
     ingeniería.

## Comprobaciones

Las pruebas automáticas de `tp-cad` comprueban sobre el rodete de referencia:

| Comprobación | Resultado |
|---|---|
| Volumen del canal frente al teorema de Pappus sobre el mismo contorno | dentro del 0,2 % |
| Fluido + álabes recortados = canal (conservación en las booleanas) | dentro de 10⁻⁴ (medido: 2·10⁻⁶) |
| Número de sólidos | fluido 1, rodete 1, un sólido por álabe |
| Validez | `BRepCheck_Analyzer` de álabe, canal, fluido y rodete |
| Volumen de cada álabe frente a longitud × ancho × espesor | dentro del ±30 % |
| STEP | se escribe y se vuelve a leer con el mismo volumen; contiene `B_SPLINE_SURFACE` |

**Volúmenes.** Se integran de forma adaptativa (tolerancia 10⁻³, error real cercano a
10⁻⁵). La regla de Gauss simple es instantánea, pero se equivoca un 2 % en sólidos con
muchas caras B-spline. El volumen sobre la malla depende de la triangulación de cada
copia y lo descartamos.

## Exportación

`turbopump cad proyecto.tpj --format step,iges,stl` escribe `impeller.*` y `fluid.*`:

| Formato | Contenido |
|---|---|
| STEP AP214 | B-rep en mm |
| IGES | B-rep (modo 1) en mm |
| STL | malla con flecha d₂/2000 |

El informe incluye el volumen del rodete, su masa (densidad por defecto 7850 kg/m³) y el
volumen del dominio fluido.

## Limitaciones

- El disco del cubo es macizo hasta el eje: no hay alas, buje ni alivios. Por eso la
  masa sale alta (2,7 kg en la referencia). El acabado (redondeos, recorte, agujeros de
  equilibrado) es de la Fase 9.
- El dominio fluido no tiene prolongaciones de entrada ni de salida (geometría virtual
  de la Fase 8).
- Construir el modelo con las booleanas y los volúmenes tarda unos 30 s.
