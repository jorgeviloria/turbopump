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
4. **Rodete** = (perfil de revolución − canal) ∪ (álabes ∩ canal), menos los agujeros
   de equilibrado.
   - **Perfil de revolución:** en un solo contorno, el disco del cubo, el canal y la
     tapa. Restarle el canal deja las dos paredes con las mismas caras que el canal, y
     los álabes recortados las comparten; así la unión no trabaja con caras casi
     coincidentes.
   - **Disco del cubo (hueco):** una pared del espesor de pared detrás del contorno del
     cubo, desde d₂ hasta un manguito alrededor del agujero del eje; entre la pared y
     el manguito, la parte trasera queda vacía. El manguito llega hasta la cara trasera
     y su diámetro exterior es, por defecto, el agujero más una pared de un cuarto de
     él, no más fina que la pared (elección propia; 28,9 mm con un eje de 19,3 mm).
     El agujero del eje es por defecto el diámetro del cubo en el ojo.
   - **Tapa:** el contorno de la tapa con espesor hacia fuera.
   - **Espesor de pared por defecto:** max(3 mm; 0,02·d₂). Es un valor por defecto de
     ingeniería.
5. **Redondeos** donde los álabes tocan el cubo y la tapa, con radio por defecto de
   medio espesor de álabe (elección propia):
   - las aristas se eligen por su forma: corren sobre el contorno meridional del cubo o
     de la tapa (a menos de 0,05 mm) y cambian a la vez de radio y de ángulo, al
     contrario que los círculos y costuras de las propias paredes;
   - se redondean con `BRepFilletAPI_MakeFillet` en el rodete (aristas cóncavas) y, con
     el mismo radio, en el dominio fluido (aristas convexas). Una booleana entre los dos
     sólidos redondeados encuentra sus caras comunes y falla, así que no se usa;
   - si el redondeo falla, el modelo sale con aristas vivas y un aviso.
6. **Agujeros de equilibrado** (con `--balance-holes` o la casilla de la interfaz): uno
   por canal, del diámetro del empuje axial (Gülich cap. 9.2.2, ver
   [`prestaciones.md`](prestaciones.md)). Son taladros paralelos al eje en un círculo
   a medio camino entre el manguito y el anillo trasero $d_{s2}$, cada uno a mitad de
   paso entre dos álabes, desde detrás del disco hasta pasada la superficie del cubo
   (disposición propia).

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
| Redondeos frente a $(1 - \pi/4)\,r^2$ a cada lado de cada raíz | entre 0,7 y 2 veces; el rodete gana lo que pierde el fluido |
| Agujeros frente a un taladro a través de la pared | entre 1 y 3 veces por agujero |

**Volúmenes.** Se integran de forma adaptativa (tolerancia 10⁻³, error real cercano a
10⁻⁵). La regla de Gauss simple es instantánea, pero se equivoca: un 2 % en sólidos con
muchas caras B-spline y hasta un 11 % en el rodete terminado (199 cm³ en lugar de
225 cm³). Con ella los redondeos parecían quitar 74 cm³ al fluido; quitan 1,6 cm³. El
volumen sobre la malla depende de la triangulación de cada copia y lo descartamos.

## Exportación

`turbopump cad proyecto.tpj --format step,iges,stl` escribe `impeller.*` y `fluid.*`.
Las opciones `--wall`, `--bore`, `--boss`, `--fillet` y `--balance-holes` fijan el
espesor de pared, el agujero del eje, el manguito, el radio de redondeo y los agujeros
de equilibrado:

| Formato | Contenido |
|---|---|
| STEP AP214 | B-rep en mm |
| IGES | B-rep (modo 1) en mm |
| STL | malla con flecha d₂/2000 |

El informe incluye el volumen del rodete, su masa (densidad por defecto 7850 kg/m³), el
volumen del dominio fluido, el agujero y el manguito, el radio de los redondeos y los
agujeros de equilibrado.

**Rodete de referencia:**

| Modelo | Rodete | Masa (acero) | Fluido |
|---|---|---|---|
| Aristas vivas | 224,8 cm³ | 1,76 kg | 438,4 cm³ |
| Redondeos de 1,5 mm | 226,4 cm³ | 1,78 kg | 436,8 cm³ |
| Redondeos y 7 agujeros de 8,5 mm | 224,4 cm³ | 1,76 kg | 436,8 cm³ |

Con el disco macizo de antes el rodete pesaba 2,7 kg.

## Limitaciones

- El manguito acaba en la cara trasera del disco: no hay chavetero, ni buje que
  sobresalga, ni anillo trasero dibujado. El radio de los redondeos es constante, igual
  en el cubo y en la tapa. Los agujeros de equilibrado son rectos y paralelos al eje.
- El recorte del rodete (d₂ menor) está en las prestaciones, no en el CAD.
- El dominio fluido no tiene prolongaciones de entrada ni de salida (geometría virtual
  de la Fase 8).
- Construir el modelo con las booleanas tarda unos 15 s, y los volúmenes exactos unos
  segundos más.
