# Cómo contribuir

## Reglas de sala limpia

Turbopump reimplementa desde cero funciones que existen en programas comerciales de
diseño de turbomáquinas. Para que el proyecto pueda seguir siendo libre, toda
contribución tiene que cumplir estas reglas:

1. **No decompilar, desensamblar ni analizar binarios** de programas comerciales, ni usar
   resultados de terceros que lo hayan hecho.
2. **No copiar** textos, figuras, iconos, tablas de datos ni código de manuales o
   programas propietarios. Leer un manual público para saber *qué* hace una función está
   permitido. Copiar *cómo* lo explica o lo implementa, no.
3. **Citar la fuente** de cada ecuación o correlación en el comentario de documentación
   de la función que la implementa: autor, obra, edición y, si es posible, número de
   ecuación o página.
4. **No subir** instaladores, manuales ni archivos de proyecto de programas propietarios.
   El `.gitignore` ya los excluye.

Una pull request que no cumpla estas reglas se rechaza.

## Estilo

- El código, los identificadores y la documentación de la API van en inglés. Los
  documentos para usuarios pueden ir en español o en inglés.
- Todas las magnitudes internas se guardan en **SI**. Las unidades de usuario solo se usan
  al leer valores y al mostrarlos (crate `tp-units`).
- Antes de abrir una pull request, ejecuta:

  ```bash
  cargo fmt --all
  ```

  ```bash
  cargo clippy --workspace --all-targets -- -D warnings
  ```

  ```bash
  cargo test --workspace
  ```

- Todo cálculo nuevo necesita un test contra un valor publicado: un ejemplo resuelto de un
  libro o un benchmark público.
