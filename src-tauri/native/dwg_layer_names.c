/* DWG helper: correct UTF-8 layer table + entity→layer_idx map.
 *
 * LibreDWG 0.14's GeoJSON writer emits layer.name as raw UTF-16LE bytes
 * re-interpreted as Latin1 — irreversible mojibake.  The C API holds the
 * same bytes; we decode them here to proper UTF-8 once.
 *
 * Output JSON:
 *   { "layers": [ { "idx", "name", "aci", "off" }, ... ],
 *     "entity_layer": { "handle_hex": idx, ... } }
 *
 * Matching entity → layer: walk the LAYER object table, build a handle-value
 * → idx table, then for each entity emit `{ entity.handle.value_ref_hex : idx }`.
 */
#include <dwg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void emit_utf16le_json(const char *buf, int cap) {
    int i = 0;
    while (i + 1 < cap && !(buf[i] == 0 && buf[i + 1] == 0)) {
        unsigned c = (unsigned char)buf[i] | ((unsigned char)buf[i+1] << 8);
        i += 2;
        if (c >= 0xD800 && c <= 0xDBFF && i + 1 < cap) {
            unsigned lo = (unsigned char)buf[i] | ((unsigned char)buf[i+1] << 8);
            if (lo >= 0xDC00 && lo <= 0xDFFF) {
                c = 0x10000 + ((c - 0xD800) << 10) + (lo - 0xDC00);
                i += 2;
            }
        }
        if      (c < 0x80)   { putchar((char)c); }
        else if (c < 0x800)  { putchar((char)(0xC0 | (c >> 6)));
                               putchar((char)(0x80 | (c & 0x3F))); }
        else if (c < 0x10000){ putchar((char)(0xE0 | (c >> 12)));
                               putchar((char)(0x80 | ((c >> 6) & 0x3F)));
                               putchar((char)(0x80 | (c & 0x3F))); }
        else                 { putchar((char)(0xF0 |  (c >> 18)));
                               putchar((char)(0x80 | ((c >> 12) & 0x3F)));
                               putchar((char)(0x80 | ((c >> 6)  & 0x3F)));
                               putchar((char)(0x80 |  (c & 0x3F))); }
    }
}

int main(int argc, char **argv) {
    if (argc != 2) return 1;
    Dwg_Data dwg;
    memset(&dwg, 0, sizeof(dwg));
    if (dwg_read_file(argv[1], &dwg)) {
        fprintf(stderr, "dwg_read_file failed\n");
        return 2;
    }

    unsigned nl = dwg_get_layer_count(&dwg);
    Dwg_Object_LAYER **L = dwg_get_layers(&dwg);

    /* Build a handle-value → idx map for LAYER objects.  We key by the
     * LAYER object's .handle.value so we can later look up an entity's
     * layer via its .layer->handleref.value. */
    typedef struct { unsigned long value; long idx; } HL;
    HL *ht = calloc(nl + 1, sizeof(HL));
    long nht = 0;
    /* Walk the object table; LAYER objects appear in dense order matching
     * the dwg_get_layers() array. */
    for (unsigned i = 0; i < dwg.num_objects; i++) {
        Dwg_Object *o = &dwg.object[i];
        if (o->type != DWG_TYPE_LAYER) continue;
        if (nht >= (long)nl) break;
        ht[nht].value = (unsigned long)o->handle.value;
        ht[nht].idx    = (long)nht;
        nht++;
    }

    /* LAYER table with correct UTF-8 names. */
    printf("{\"layers\":[");
    for (unsigned i = 0; i < nl; i++) {
        if (i) putchar(',');
        Dwg_Object_LAYER *l = L ? L[i] : NULL;
        printf("{\"idx\":%u,\"name\":\"", i);
        if (l && l->name) emit_utf16le_json(l->name, 1 << 16);
        printf("\",\"aci\":%d,\"off\":%u}", l ? l->color.index : 256,
               l ? (unsigned)l->off : 0);
    }
    printf("],\"entity_layer\":{");

    int first = 1;
    for (unsigned j = 0; j < dwg.num_objects; j++) {
        Dwg_Object *o = &dwg.object[j];
        if (o->supertype != DWG_SUPERTYPE_ENTITY) continue;
        Dwg_Object_Entity *e = o->tio.entity;
        if (!e) continue;

        long lix = -1;
        if (e->layer) {
            unsigned long layer_h = 0;
            /* handleref.value is the canonical layer handle we see in handleref code */
            layer_h = (unsigned long)e->layer->handleref.value;
            for (long k = 0; k < nht; k++) {
                if (ht[k].value == layer_h) { lix = ht[k].idx; break; }
            }
        }
        if (!first) putchar(',');
        first = 0;
        printf("\"%lX\":%ld", (unsigned long)o->handle.value, lix);
    }
    printf("}}\n");

    free(ht);
    dwg_free(&dwg);
    return 0;
}
