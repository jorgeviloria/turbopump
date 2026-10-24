# Plantilla de ensayo

`ensayo-bomba.xlsx` sirve para pasar a Turbopump una bomba ensayada: la geometría
principal y los puntos medidos de la curva. Los valores que trae son un ejemplo inventado
con números verosímiles; no vienen de ninguna bomba real.

- Hoja **Datos**: se rellena y se exporta sola a CSV («Guardar como → CSV UTF-8»).
- Hoja **Instrucciones**: cómo rellenarla y qué pasa con los datos.

`ensayo-bomba-ejemplo.csv` es lo que da esa exportación con el ejemplo, y
`generar.py` vuelve a crear los dos archivos (`python3 generar.py ensayo-bomba.xlsx`,
con `openpyxl`).

## Formato del CSV

El lector de ensayos que se añadirá a Turbopump lee el CSV, no el `.xlsx`, para no
depender de una biblioteca de hojas de cálculo. Lo que espera:

- **Separador** coma o punto y coma, y **decimal** punto o coma: la versión española de
  Excel exporta con `;` y coma decimal.
- Las filas vacías y las que no reconoce se ignoran. Las secciones empiezan en una celda
  `[BOMBA]` o `[ENSAYO]` de la primera columna.
- **`[BOMBA]`**: una cabecera `clave,valor,unidad,descripción` y una fila por dato. Solo
  cuentan la clave, el valor y la unidad. Una clave que falta, o con el valor vacío, es un
  dato que no se tiene.
- **`[ENSAYO]`**: una fila de nombres (`caudal`, `altura`, `potencia`, `rendimiento`,
  `npsh3`, en cualquier orden), una de unidades y una de descripciones, y luego una fila
  por punto medido. Una columna vacía es una magnitud que no se midió.

| Clave | Unidades | Obligatoria |
|---|---|---|
| `nombre`, `fluido`, `aspiracion` (`simple`/`doble`), `voluta` (`simple`/`doble`) | — | sí |
| `caudal` | m3/h, l/s, m3/s, gpm | sí |
| `altura` | m, ft | sí |
| `velocidad`, `velocidad_ensayo` | rpm | sí |
| `temperatura` | °C, K | sí |
| `presion_entrada` | kPa, bar, Pa | no |
| `d2`, `b2`, `d1`, `d_cubo`, `dz`, `b3` | mm, in | sí |
| `espesor`, `d_anillo`, `holgura_anillo` | mm, in | no |
| `z` | — | sí |
| `beta2` | ° | sí |
| `garganta` | mm2, in2 | no |
| `nervio` | ° | no |
| `norma`, `fecha`, `banco` | — | no |

Columnas del ensayo: `caudal` (m3/h, l/s, gpm), `altura` (m, ft), `potencia` en el eje
(kW, hp), `rendimiento` total (% o fracción) y `npsh3` (m, ft). Si `velocidad_ensayo`
difiere de `velocidad`, los puntos se llevan a la nominal por semejanza.

## Datos de fábrica

Los ensayos y las geometrías reales son de la empresa que los mide. Van en
`tests/benchmarks/privado/`, que no entra en el repositorio, y solo con permiso. Sirven
para comparar la predicción con la medida, nunca para ajustar coeficientes a una bomba
concreta.
