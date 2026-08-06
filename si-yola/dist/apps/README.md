# YOLA App Store — Catálogo local (fallback offline)

`catalog.json` es el **índice de referencia**: las apps de la comunidad
SON productos independientes — su código vive en sus propios repos de
GitHub, no aquí. Este catálogo solo menciona que existen.

## Lo que NO vive aquí

- ❌ Código de apps (el de YOLA Code está en `github.com/SayriDevs/YolaCode`)
- ❌ Manifests de apps (se descargan del repo vía el bridge)
- ❌ Bundles compilados

## Lo que SÍ vive aquí

```json
{
  "id": "yola-code",
  "name": "YOLA Code",
  "author": "YOLA",
  "repo": "github.com/SayriDevs/YolaCode",
  "manifestUrl": "https://cdn.jsdelivr.net/gh/SayriDevs/YolaCode@main/manifest.json"
}
```

El App Store:
1. Descarga el catálogo (daemon → remoto `yolabysayri.com` → este local)
2. Verifica la app contra su repo (identidad + checksum vía bridge)
3. Instala el manifest + entry **desde el repo** (fresco, sin caché CDN)

## Añadir una app de la comunidad

1. El creador publica su repo con `manifest.json` + entry
2. Añade su entrada aquí (y en el catálogo remoto de `yolabysayri.com`)
3. Listo — el OS hace el resto (verificar, instalar, actualizar, fork)
