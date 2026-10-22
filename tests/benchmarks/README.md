# Casos de validación

El resumen de lo comparado está en [docs/validation/](../../docs/validation/).

Cada carpeta es un caso de referencia con dos archivos:

- `caso.tpj`: el proyecto de Turbopump con las entradas del caso;
- `targets.json`: los valores de referencia, cada uno con su tolerancia.

`turbopump validate caso.tpj targets.json` calcula cada magnitud y la compara con su
referencia. Termina con código 0 si todas las que tienen tolerancia o banda la cumplen y
con 2 si alguna queda fuera. La prueba `crates/tp-project/tests/benchmarks.rs` hace lo
mismo con cada caso.

Coincidir dentro de una tolerancia significa «comparado con, dentro de ±x», no
«validado» ni «conforme». Las tolerancias de las normas de ensayo de aceptación se aplican
a bombas medidas: una predicción de diseño preliminar no certifica nada.

## Formato de `targets.json`

```json
{
  "format": "turbopump-targets",
  "version": 1,
  "case": "nombre corto",
  "description": "una línea",
  "notes": ["procedencia, por qué esas tolerancias, diferencias conocidas"],
  "targets": [
    {"id": "b2", "value": 17.0, "unit": "mm", "tolerance": {"relative": 3}},
    {"id": "eta", "value": 75.9, "unit": "%", "tolerance": {"absolute": 2}},
    {"id": "beta1B@hub", "value": 44.4, "unit": "°", "tolerance": {"absolute": 1}},
    {"id": "npsh3", "band": [1.9, 5.12], "unit": "m"},
    {"id": "slip", "value": 0.807, "note": "sin tolerancia: solo informativa"}
  ]
}
```

- `id`: una de las magnitudes de `turbopump validate --list`. Las que se dan por sección
  del álabe llevan `@span`: `hub`, `mid`, `shroud` o una fracción de 0 a 1 que coincida
  con una sección.
- `value` o `band`: un valor de referencia o un intervalo en el que debe caer el
  resultado.
- `unit`: la unidad de `value`, `band` y de una tolerancia absoluta; se omite en los
  números sin dimensión.
- `tolerance`: `relative` en % del valor, o `absolute` en la unidad del objetivo (con `%`,
  en puntos). Sin tolerancia, la magnitud solo se muestra.

Algunas magnitudes tienen dos versiones porque las referencias las definen de forma
distinta. `cm2`, `cm1` y `beta2` usan el caudal del rodete $Q_{La} = Q/\eta_v$, y
`cm2_q`, `cm1_q` y `beta2_q`, el de la bomba $Q$.

## Reglas

- Las tolerancias se fijan antes de calcular y se justifican en `notes`. Si un valor no
  cabe, se queda fuera y se documenta: no se cambia un coeficiente para un caso concreto.
  Una tolerancia solo se quita si la magnitud de la referencia resulta no ser la misma.
  Ese cambio se explica en `notes`.
- Los casos públicos salen de ejemplos publicados y citan obra, edición y figura o página.
- Los casos con datos de terceros que no se pueden publicar van en `privado/`. Esa
  carpeta se excluye en `.git/info/exclude` de cada copia local, así que nunca entra en el
  repositorio. La prueba ejecuta esos casos si existen.

## Casos

| Caso | Fuente | Resultado |
|---|---|---|
| [japikse-7.5](japikse-7.5/) | Japikse, Marscher y Furst, *Centrifugal Pump Design and Performance*, 1997, cap. 7.5 | 6 dentro, 4 fuera, 5 informativas |
