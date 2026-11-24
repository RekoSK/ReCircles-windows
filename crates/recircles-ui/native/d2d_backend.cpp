#include <windows.h>
#include <shobjidl.h>
#include <shlobj.h>
#include <shellapi.h>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <d2d1_1.h>
#include <d2d1effects.h>
#include <dcomp.h>
#include <dwmapi.h>
#include <dwrite_1.h>
#include <wincodec.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <cwctype>
#include <list>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

using Microsoft::WRL::ComPtr;

namespace {

constexpr float kPi = 3.14159265358979323846f;
constexpr float kTau = 6.28318530717958647692f;

struct D2dColor {
    float r;
    float g;
    float b;
    float a;
};

struct D2dGradStop {
    float pos;
    D2dColor color;
};

// Path commands for generic vector icons & SVGs
// op: 0 = MoveTo(x1,y1), 1 = LineTo(x1,y1), 2 = CubicTo(x1,y1,x2,y2,x3,y3),
//     3 = Arc(cx=x1, cy=y1, r=x2, start_rad=y2, end_rad=x3),
//     4 = CloseFigure,
//     5 = RoundedRect(x=x1, y=y1, w=x2, h=y2, r=x3)
struct D2dPathCmd {
    uint32_t op;
    float x1, y1;
    float x2, y2;
    float x3, y3;
};

inline D2D1_COLOR_F to_d2d(D2dColor c) {
    return D2D1::ColorF(c.r, c.g, c.b, c.a);
}

struct FontKey {
    uint32_t size_tenths;
    uint8_t bold;
    uint8_t align;
    uint8_t wrap;
    bool operator==(const FontKey& o) const {
        return size_tenths == o.size_tenths && bold == o.bold && align == o.align && wrap == o.wrap;
    }
};

struct FontKeyHash {
    size_t operator()(const FontKey& k) const noexcept {
        return (static_cast<size_t>(k.size_tenths) << 8) ^
               (static_cast<size_t>(k.bold) << 4) ^
               (static_cast<size_t>(k.align) << 2) ^
               static_cast<size_t>(k.wrap);
    }
};

struct CachedFormat {
    ComPtr<IDWriteTextFormat> format;
    ComPtr<IDWriteInlineObject> ellipsis;
};

struct TextLayoutKey {
    std::wstring text;
    uint32_t size_tenths = 0;
    uint32_t w_half_px = 0;
    uint32_t h_half_px = 0;
    uint8_t bold = 0;
    uint8_t align = 0;
    uint8_t ellipsize = 0;
    uint8_t wrap_lines = 1;

    bool operator==(const TextLayoutKey& o) const {
        return size_tenths == o.size_tenths && w_half_px == o.w_half_px &&
               h_half_px == o.h_half_px && bold == o.bold && align == o.align &&
               ellipsize == o.ellipsize && wrap_lines == o.wrap_lines && text == o.text;
    }
};

struct TextLayoutKeyHash {
    size_t operator()(const TextLayoutKey& k) const noexcept {
        size_t h = 14695981039346656037ULL;
        auto mix = [&](size_t v) {
            h ^= v;
            h *= 1099511628211ULL;
        };
        mix(k.size_tenths);
        mix(k.w_half_px);
        mix(k.h_half_px);
        mix((static_cast<size_t>(k.bold) << 24) |
            (static_cast<size_t>(k.align) << 16) |
            (static_cast<size_t>(k.ellipsize) << 8) |
            static_cast<size_t>(k.wrap_lines));
        for (wchar_t ch : k.text) {
            mix(static_cast<size_t>(ch));
        }
        return h;
    }
};

struct AnnulusKey {
    uint32_t inner_tenths;
    uint32_t outer_tenths;
    bool operator==(const AnnulusKey& o) const {
        return inner_tenths == o.inner_tenths && outer_tenths == o.outer_tenths;
    }
};

struct AnnulusKeyHash {
    size_t operator()(const AnnulusKey& k) const noexcept {
        return (static_cast<size_t>(k.inner_tenths) * 2654435761ULL) ^
               (static_cast<size_t>(k.outer_tenths) << 16);
    }
};

struct RRectHoleKey {
    int32_t x_t;
    int32_t y_t;
    uint32_t w_t;
    uint32_t h_t;
    uint32_t r_t;
    uint32_t hole_t;
    bool operator==(const RRectHoleKey& o) const {
        return x_t == o.x_t && y_t == o.y_t && w_t == o.w_t &&
               h_t == o.h_t && r_t == o.r_t && hole_t == o.hole_t;
    }
};

struct RRectHoleKeyHash {
    size_t operator()(const RRectHoleKey& k) const noexcept {
        size_t h = 14695981039346656037ULL;
        auto mix = [&](size_t v) {
            h ^= v;
            h *= 1099511628211ULL;
        };
        mix(static_cast<size_t>(static_cast<uint32_t>(k.x_t)));
        mix(static_cast<size_t>(static_cast<uint32_t>(k.y_t)));
        mix(k.w_t);
        mix(k.h_t);
        mix(k.r_t);
        mix(k.hole_t);
        return h;
    }
};

struct MeasureKey {
    std::wstring text;
    uint32_t size_tenths = 0;
    uint32_t max_w_half = 0;
    int32_t max_lines = 1;

    bool operator==(const MeasureKey& o) const {
        return size_tenths == o.size_tenths && max_w_half == o.max_w_half &&
               max_lines == o.max_lines && text == o.text;
    }
};

struct MeasureKeyHash {
    size_t operator()(const MeasureKey& k) const noexcept {
        size_t h = 14695981039346656037ULL;
        auto mix = [&](size_t v) {
            h ^= v;
            h *= 1099511628211ULL;
        };
        mix(k.size_tenths);
        mix(k.max_w_half);
        mix(static_cast<size_t>(static_cast<uint32_t>(k.max_lines)));
        for (wchar_t ch : k.text) {
            mix(static_cast<size_t>(ch));
        }
        return h;
    }
};

struct MeasureVal {
    float w = 0.0f;
    float h = 0.0f;
};

struct LinearBrushKey {
    uint8_t count = 0;
    // Packed quantized stops: [pos, r, g, b, norm_a] * 8 (zero struct padding)
    std::array<uint16_t, 40> data{};

    bool operator==(const LinearBrushKey& o) const {
        if (count != o.count) return false;
        return std::memcmp(data.data(), o.data.data(), static_cast<size_t>(count) * 5 * sizeof(uint16_t)) == 0;
    }
};

struct LinearBrushKeyHash {
    size_t operator()(const LinearBrushKey& k) const noexcept {
        size_t h = 14695981039346656037ULL;
        auto mix = [&](uint32_t v) {
            h ^= static_cast<size_t>(v);
            h *= 1099511628211ULL;
        };
        mix(k.count);
        size_t n = static_cast<size_t>(k.count) * 5;
        for (size_t i = 0; i < n; ++i) {
            mix(k.data[i]);
        }
        return h;
    }
};

struct SheenKey {
    int32_t x_t;
    int32_t y_t;
    uint32_t w_t;
    uint32_t h_t;
    uint32_t r_t;
    uint32_t sh_t;
    bool operator==(const SheenKey& o) const {
        return x_t == o.x_t && y_t == o.y_t && w_t == o.w_t &&
               h_t == o.h_t && r_t == o.r_t && sh_t == o.sh_t;
    }
};

struct SheenKeyHash {
    size_t operator()(const SheenKey& k) const noexcept {
        size_t h = 14695981039346656037ULL;
        auto mix = [&](size_t v) {
            h ^= v;
            h *= 1099511628211ULL;
        };
        mix(static_cast<size_t>(static_cast<uint32_t>(k.x_t)));
        mix(static_cast<size_t>(static_cast<uint32_t>(k.y_t)));
        mix(k.w_t);
        mix(k.h_t);
        mix(k.r_t);
        mix(k.sh_t);
        return h;
    }
};

struct ArcKey {
    int32_t cx_t;
    int32_t cy_t;
    uint32_t r_t;
    int32_t s_mrad;
    int32_t e_mrad;
    bool operator==(const ArcKey& o) const {
        return cx_t == o.cx_t && cy_t == o.cy_t && r_t == o.r_t &&
               s_mrad == o.s_mrad && e_mrad == o.e_mrad;
    }
};

struct ArcKeyHash {
    size_t operator()(const ArcKey& k) const noexcept {
        size_t h = 14695981039346656037ULL;
        auto mix = [&](size_t v) {
            h ^= v;
            h *= 1099511628211ULL;
        };
        mix(static_cast<size_t>(static_cast<uint32_t>(k.cx_t)));
        mix(static_cast<size_t>(static_cast<uint32_t>(k.cy_t)));
        mix(k.r_t);
        mix(static_cast<size_t>(static_cast<uint32_t>(k.s_mrad)));
        mix(static_cast<size_t>(static_cast<uint32_t>(k.e_mrad)));
        return h;
    }
};

struct PathKey {
    uint8_t filled = 0;
    std::vector<D2dPathCmd> cmds;
    bool operator==(const PathKey& o) const {
        return filled == o.filled && cmds.size() == o.cmds.size() &&
               (cmds.empty() || std::memcmp(cmds.data(), o.cmds.data(), cmds.size() * sizeof(D2dPathCmd)) == 0);
    }
};

struct PathKeyHash {
    size_t operator()(const PathKey& k) const noexcept {
        size_t h = 14695981039346656037ULL;
        auto mix = [&](uint32_t v) {
            h ^= static_cast<size_t>(v);
            h *= 1099511628211ULL;
        };
        mix(k.filled);
        mix(static_cast<uint32_t>(k.cmds.size()));
        const uint32_t* words = reinterpret_cast<const uint32_t*>(k.cmds.data());
        size_t word_count = k.cmds.size() * (sizeof(D2dPathCmd) / sizeof(uint32_t));
        for (size_t i = 0; i < word_count; ++i) {
            mix(words[i]);
        }
        return h;
    }
};

constexpr size_t kMaxLayoutCacheEntries = 512;
constexpr size_t kMaxMeasureCacheEntries = 256;
constexpr size_t kMaxLinearBrushCacheEntries = 128;
constexpr size_t kMaxAnnulusCacheEntries = 64;
constexpr size_t kMaxRRectCacheEntries = 128;
constexpr size_t kMaxShadowCacheEntries = 96;
constexpr size_t kMaxSheenCacheEntries = 64;
constexpr size_t kMaxArcCacheEntries = 64;
constexpr size_t kMaxPathCacheEntries = 128;

using LayoutLruList = std::list<std::pair<TextLayoutKey, ComPtr<IDWriteTextLayout>>>;

struct D2dCtx {
    HWND hwnd = nullptr;
    int phys_w = 1;
    int phys_h = 1;
    float logical_scale = 1.0f;
    bool com_inited = false;

    ComPtr<ID3D11Device> d3d_dev;
    ComPtr<IDXGIDevice> dxgi_dev;
    ComPtr<IDXGISwapChain1> swapchain;
    ComPtr<ID2D1Factory1> d2d_factory;
    ComPtr<ID2D1Device> d2d_dev;
    ComPtr<ID2D1DeviceContext> d2d_ctx;
    ComPtr<ID2D1Bitmap1> target_bmp;

    ComPtr<IDCompositionDevice> dcomp_dev;
    ComPtr<IDCompositionTarget> dcomp_target;
    ComPtr<IDCompositionVisual> dcomp_visual;

    ComPtr<IDWriteFactory> dwrite;
    ComPtr<IDWriteTypography> tabular_typography;
    ComPtr<IWICImagingFactory> wic;

    ComPtr<ID2D1SolidColorBrush> solid_brush;
    ComPtr<ID2D1SolidColorBrush> ghost_brush;
    ComPtr<ID2D1StrokeStyle> round_stroke;
    ComPtr<ID2D1StrokeStyle> flat_stroke;

    // GPU-blurred frosted desktop backdrop (raw_backdrop retained for 0-latency live blur slider)
    ComPtr<ID2D1Bitmap1> raw_backdrop;
    ComPtr<ID2D1Bitmap1> blurred_backdrop;
    ComPtr<ID2D1BitmapBrush1> backdrop_brush;
    int backdrop_w = 0;
    int backdrop_h = 0;
    float backdrop_origin_x = 0.0f;
    float backdrop_origin_y = 0.0f;
    float last_blur_sigma = -1.0f;
    float last_blur_sat = -1.0f;
    bool has_backdrop = false;

    std::wstring font_family = L"Segoe UI";
    std::unordered_map<FontKey, CachedFormat, FontKeyHash> format_cache;
    LayoutLruList layout_lru;
    std::unordered_map<TextLayoutKey, LayoutLruList::iterator, TextLayoutKeyHash> layout_map;
    TextLayoutKey layout_probe_key;
    std::unordered_map<MeasureKey, MeasureVal, MeasureKeyHash> measure_cache;
    MeasureKey measure_probe_key;

    std::unordered_map<LinearBrushKey, ComPtr<ID2D1LinearGradientBrush>, LinearBrushKeyHash> linear_brush_cache;
    std::unordered_map<AnnulusKey, ComPtr<ID2D1PathGeometry>, AnnulusKeyHash> annulus_cache;
    std::unordered_map<RRectHoleKey, ComPtr<ID2D1PathGeometry>, RRectHoleKeyHash> rrect_hole_cache;
    std::unordered_map<RRectHoleKey, std::array<ComPtr<ID2D1PathGeometry>, 3>, RRectHoleKeyHash> shadow_cache;
    std::unordered_map<SheenKey, ComPtr<ID2D1PathGeometry>, SheenKeyHash> sheen_cache;
    std::unordered_map<ArcKey, ComPtr<ID2D1PathGeometry>, ArcKeyHash> arc_cache;
    std::unordered_map<PathKey, ComPtr<ID2D1PathGeometry>, PathKeyHash> path_cache;
    PathKey path_probe_key;

    std::vector<D2D1_MATRIX_3X2_F> transform_stack;

    int next_bmp_id = 1;
    std::unordered_map<int, ComPtr<ID2D1Bitmap1>> bitmaps;
};

bool bind_swapchain_target(D2dCtx* ctx) {
    ctx->d2d_ctx->SetTarget(nullptr);
    ctx->target_bmp.Reset();

    ComPtr<IDXGISurface> surface;
    HRESULT hr = ctx->swapchain->GetBuffer(0, IID_PPV_ARGS(&surface));
    if (FAILED(hr)) return false;

    D2D1_BITMAP_PROPERTIES1 props = D2D1::BitmapProperties1(
        D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED),
        96.0f * ctx->logical_scale,
        96.0f * ctx->logical_scale);

    hr = ctx->d2d_ctx->CreateBitmapFromDxgiSurface(surface.Get(), &props, &ctx->target_bmp);
    if (FAILED(hr)) return false;
    ctx->d2d_ctx->SetTarget(ctx->target_bmp.Get());
    return true;
}

void pick_best_font(D2dCtx* ctx) {
    if (!ctx->dwrite) return;
    ComPtr<IDWriteFontCollection> coll;
    if (FAILED(ctx->dwrite->GetSystemFontCollection(&coll))) return;
    const wchar_t* candidates[] = {
        L"Segoe UI Variable Display",
        L"Segoe UI Variable Text",
        L"Segoe UI",
    };
    for (const wchar_t* name : candidates) {
        UINT32 idx = 0;
        BOOL exists = FALSE;
        if (SUCCEEDED(coll->FindFamilyName(name, &idx, &exists)) && exists) {
            ctx->font_family = name;
            return;
        }
    }
}

const CachedFormat* get_text_format_entry(D2dCtx* ctx, float size, bool bold, int align, bool wrap) {
    uint32_t q = static_cast<uint32_t>(std::clamp(std::round(size * 10.0f), 50.0f, 1600.0f));
    FontKey key{q, static_cast<uint8_t>(bold ? 1 : 0), static_cast<uint8_t>(align), static_cast<uint8_t>(wrap ? 1 : 0)};
    auto it = ctx->format_cache.find(key);
    if (it != ctx->format_cache.end()) {
        return &it->second;
    }
    if (ctx->format_cache.size() >= 256) {
        ctx->format_cache.clear();
    }
    ComPtr<IDWriteTextFormat> fmt;
    HRESULT hr = ctx->dwrite->CreateTextFormat(
        ctx->font_family.c_str(),
        nullptr,
        bold ? DWRITE_FONT_WEIGHT_SEMI_BOLD : DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        static_cast<float>(q) * 0.1f,
        L"",
        &fmt);
    if (FAILED(hr) || !fmt) return nullptr;

    DWRITE_TEXT_ALIGNMENT ta = DWRITE_TEXT_ALIGNMENT_LEADING;
    if (align == 1) ta = DWRITE_TEXT_ALIGNMENT_CENTER;
    else if (align == 2) ta = DWRITE_TEXT_ALIGNMENT_TRAILING;
    fmt->SetTextAlignment(ta);
    fmt->SetParagraphAlignment(wrap ? DWRITE_PARAGRAPH_ALIGNMENT_NEAR : DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    fmt->SetWordWrapping(wrap ? DWRITE_WORD_WRAPPING_WRAP : DWRITE_WORD_WRAPPING_NO_WRAP);

    ComPtr<IDWriteInlineObject> ellipsis;
    if (!wrap) {
        ctx->dwrite->CreateEllipsisTrimmingSign(fmt.Get(), &ellipsis);
    }

    auto [inserted, _] = ctx->format_cache.emplace(key, CachedFormat{std::move(fmt), std::move(ellipsis)});
    return &inserted->second;
}

IDWriteTextFormat* get_text_format(D2dCtx* ctx, float size, bool bold, int align, bool wrap) {
    const CachedFormat* entry = get_text_format_entry(ctx, size, bold, align, wrap);
    return entry ? entry->format.Get() : nullptr;
}

bool create_tabular_text_layout(
    D2dCtx* ctx, const wchar_t* str, UINT32 len, IDWriteTextFormat* fmt,
    float max_w, float max_h, ComPtr<IDWriteTextLayout>& out_layout) {
    if (!ctx || !ctx->dwrite || !fmt) return false;
    if (FAILED(ctx->dwrite->CreateTextLayout(str, len, fmt, max_w, max_h, &out_layout)) || !out_layout) {
        return false;
    }
    if (!ctx->tabular_typography) {
        if (SUCCEEDED(ctx->dwrite->CreateTypography(&ctx->tabular_typography)) && ctx->tabular_typography) {
            DWRITE_FONT_FEATURE feat{DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, 1};
            ctx->tabular_typography->AddFontFeature(feat);
        }
    }
    if (ctx->tabular_typography && len > 0) {
        DWRITE_TEXT_RANGE range{0, len};
        out_layout->SetTypography(ctx->tabular_typography.Get(), range);
    }
    return true;
}

ComPtr<ID2D1PathGeometry> make_annulus_geometry(ID2D1Factory1* factory, float inner_r, float outer_r) {
    ComPtr<ID2D1PathGeometry> geo;
    if (FAILED(factory->CreatePathGeometry(&geo))) return nullptr;
    ComPtr<ID2D1GeometrySink> sink;
    if (FAILED(geo->Open(&sink))) return nullptr;
    sink->SetFillMode(D2D1_FILL_MODE_ALTERNATE);

    auto add_circle = [&](float r) {
        if (r <= 0.01f) return;
        sink->BeginFigure(D2D1::Point2F(r, 0.0f), D2D1_FIGURE_BEGIN_FILLED);
        sink->AddArc(D2D1::ArcSegment(
            D2D1::Point2F(-r, 0.0f), D2D1::SizeF(r, r), 0.0f,
            D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
        sink->AddArc(D2D1::ArcSegment(
            D2D1::Point2F(r, 0.0f), D2D1::SizeF(r, r), 0.0f,
            D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
        sink->EndFigure(D2D1_FIGURE_END_CLOSED);
    };
    add_circle(outer_r);
    add_circle(inner_r);
    sink->Close();
    return geo;
}

ID2D1PathGeometry* get_annulus_geometry(D2dCtx* ctx, float inner_r, float outer_r) {
    uint32_t in_q = static_cast<uint32_t>(std::clamp(std::round(inner_r * 10.0f), 0.0f, 100000.0f));
    uint32_t out_q = static_cast<uint32_t>(std::clamp(std::round(outer_r * 10.0f), 0.0f, 100000.0f));
    AnnulusKey key{in_q, out_q};
    auto it = ctx->annulus_cache.find(key);
    if (it != ctx->annulus_cache.end()) {
        return it->second.Get();
    }
    if (ctx->annulus_cache.size() >= kMaxAnnulusCacheEntries) {
        ctx->annulus_cache.clear();
    }
    auto geo = make_annulus_geometry(
        ctx->d2d_factory.Get(),
        static_cast<float>(in_q) * 0.1f,
        static_cast<float>(out_q) * 0.1f);
    if (!geo) return nullptr;
    auto* raw = geo.Get();
    ctx->annulus_cache.emplace(key, std::move(geo));
    return raw;
}

void add_rounded_rect_figure(ID2D1GeometrySink* sink, float x, float y, float w, float h, float r) {
    if (w <= 0.01f || h <= 0.01f) return;
    float rad = std::clamp(r, 0.0f, std::min(w, h) * 0.5f);
    if (rad <= 0.05f) {
        sink->BeginFigure(D2D1::Point2F(x, y), D2D1_FIGURE_BEGIN_FILLED);
        sink->AddLine(D2D1::Point2F(x + w, y));
        sink->AddLine(D2D1::Point2F(x + w, y + h));
        sink->AddLine(D2D1::Point2F(x, y + h));
        sink->EndFigure(D2D1_FIGURE_END_CLOSED);
        return;
    }
    // Check pure circle/ellipse case to avoid zero-length degenerate lines
    if (std::fabs(w - h) < 0.05f && std::fabs(rad - w * 0.5f) < 0.05f) {
        float cx = x + w * 0.5f;
        float cy = y + h * 0.5f;
        sink->BeginFigure(D2D1::Point2F(cx + rad, cy), D2D1_FIGURE_BEGIN_FILLED);
        sink->AddArc(D2D1::ArcSegment(
            D2D1::Point2F(cx - rad, cy), D2D1::SizeF(rad, rad), 0.0f,
            D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
        sink->AddArc(D2D1::ArcSegment(
            D2D1::Point2F(cx + rad, cy), D2D1::SizeF(rad, rad), 0.0f,
            D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
        sink->EndFigure(D2D1_FIGURE_END_CLOSED);
        return;
    }

    sink->BeginFigure(D2D1::Point2F(x + rad, y), D2D1_FIGURE_BEGIN_FILLED);
    sink->AddLine(D2D1::Point2F(x + w - rad, y));
    sink->AddArc(D2D1::ArcSegment(
        D2D1::Point2F(x + w, y + rad), D2D1::SizeF(rad, rad), 0.0f,
        D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
    sink->AddLine(D2D1::Point2F(x + w, y + h - rad));
    sink->AddArc(D2D1::ArcSegment(
        D2D1::Point2F(x + w - rad, y + h), D2D1::SizeF(rad, rad), 0.0f,
        D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
    sink->AddLine(D2D1::Point2F(x + rad, y + h));
    sink->AddArc(D2D1::ArcSegment(
        D2D1::Point2F(x, y + h - rad), D2D1::SizeF(rad, rad), 0.0f,
        D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
    sink->AddLine(D2D1::Point2F(x, y + rad));
    sink->AddArc(D2D1::ArcSegment(
        D2D1::Point2F(x + rad, y), D2D1::SizeF(rad, rad), 0.0f,
        D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
    sink->EndFigure(D2D1_FIGURE_END_CLOSED);
}

ComPtr<ID2D1PathGeometry> make_rounded_rect_hole_geometry(
    ID2D1Factory1* factory, float x, float y, float w, float h, float r, float hole_r) {
    ComPtr<ID2D1PathGeometry> geo;
    if (FAILED(factory->CreatePathGeometry(&geo))) return nullptr;
    ComPtr<ID2D1GeometrySink> sink;
    if (FAILED(geo->Open(&sink))) return nullptr;
    sink->SetFillMode(D2D1_FILL_MODE_ALTERNATE);
    add_rounded_rect_figure(sink.Get(), x, y, w, h, r);
    if (hole_r > 0.05f) {
        float cx = x + w * 0.5f;
        float cy = y + h * 0.5f;
        sink->BeginFigure(D2D1::Point2F(cx + hole_r, cy), D2D1_FIGURE_BEGIN_FILLED);
        sink->AddArc(D2D1::ArcSegment(
            D2D1::Point2F(cx - hole_r, cy), D2D1::SizeF(hole_r, hole_r), 0.0f,
            D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
        sink->AddArc(D2D1::ArcSegment(
            D2D1::Point2F(cx + hole_r, cy), D2D1::SizeF(hole_r, hole_r), 0.0f,
            D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
        sink->EndFigure(D2D1_FIGURE_END_CLOSED);
    }
    sink->Close();
    return geo;
}

ID2D1PathGeometry* get_rounded_rect_hole_geometry(
    D2dCtx* ctx, float x, float y, float w, float h, float r, float hole_r) {
    auto qt_signed = [](float v) -> int32_t {
        return static_cast<int32_t>(std::clamp(std::round(v * 10.0f), -200000.0f, 200000.0f));
    };
    auto qt_unsigned = [](float v) -> uint32_t {
        return static_cast<uint32_t>(std::clamp(std::round(v * 10.0f), 0.0f, 200000.0f));
    };
    RRectHoleKey key{
        qt_signed(x), qt_signed(y),
        qt_unsigned(w), qt_unsigned(h),
        qt_unsigned(r), qt_unsigned(hole_r),
    };
    auto it = ctx->rrect_hole_cache.find(key);
    if (it != ctx->rrect_hole_cache.end()) {
        return it->second.Get();
    }
    if (ctx->rrect_hole_cache.size() >= kMaxRRectCacheEntries) {
        ctx->rrect_hole_cache.clear();
    }
    auto geo = make_rounded_rect_hole_geometry(
        ctx->d2d_factory.Get(),
        static_cast<float>(key.x_t) * 0.1f,
        static_cast<float>(key.y_t) * 0.1f,
        static_cast<float>(key.w_t) * 0.1f,
        static_cast<float>(key.h_t) * 0.1f,
        static_cast<float>(key.r_t) * 0.1f,
        static_cast<float>(key.hole_t) * 0.1f);
    if (!geo) return nullptr;
    auto* raw = geo.Get();
    ctx->rrect_hole_cache.emplace(key, std::move(geo));
    return raw;
}

const std::array<ComPtr<ID2D1PathGeometry>, 3>* get_rounded_rect_shadow_geometries(
    D2dCtx* ctx, float x, float y, float w, float h, float r) {
    auto qt_signed = [](float v) -> int32_t {
        return static_cast<int32_t>(std::clamp(std::round(v * 10.0f), -200000.0f, 200000.0f));
    };
    auto qt_unsigned = [](float v) -> uint32_t {
        return static_cast<uint32_t>(std::clamp(std::round(v * 10.0f), 0.0f, 200000.0f));
    };
    RRectHoleKey key{
        qt_signed(x), qt_signed(y),
        qt_unsigned(w), qt_unsigned(h),
        qt_unsigned(r), 0,
    };
    auto it = ctx->shadow_cache.find(key);
    if (it != ctx->shadow_cache.end()) {
        return &it->second;
    }
    if (ctx->shadow_cache.size() >= kMaxShadowCacheEntries) {
        ctx->shadow_cache.clear();
    }
    const float pads[3] = {1.8f, 3.6f, 5.4f};
    float qx = static_cast<float>(key.x_t) * 0.1f;
    float qy = static_cast<float>(key.y_t) * 0.1f;
    float qw = static_cast<float>(key.w_t) * 0.1f;
    float qh = static_cast<float>(key.h_t) * 0.1f;
    float qr = static_cast<float>(key.r_t) * 0.1f;
    std::array<ComPtr<ID2D1PathGeometry>, 3> geos;
    for (size_t i = 0; i < 3; ++i) {
        float pad = pads[i];
        ComPtr<ID2D1PathGeometry> geo;
        if (FAILED(ctx->d2d_factory->CreatePathGeometry(&geo))) continue;
        ComPtr<ID2D1GeometrySink> sink;
        if (FAILED(geo->Open(&sink))) continue;
        sink->SetFillMode(D2D1_FILL_MODE_ALTERNATE);
        add_rounded_rect_figure(sink.Get(), qx - pad, qy + 1.0f - pad, qw + 2.0f * pad, qh + 2.0f * pad, qr + pad);
        add_rounded_rect_figure(sink.Get(), qx, qy, qw, qh, qr);
        sink->Close();
        geos[i] = std::move(geo);
    }
    auto [inserted, _] = ctx->shadow_cache.emplace(key, std::move(geos));
    return &inserted->second;
}

ComPtr<ID2D1PathGeometry> make_sector_geometry(
    ID2D1Factory1* factory, float inner_r, float outer_r, float start_rad, float end_rad) {
    float sweep = end_rad - start_rad;
    if (sweep >= kTau - 0.0005f) {
        return make_annulus_geometry(factory, inner_r, outer_r);
    }
    if (sweep <= 1e-5f || outer_r <= inner_r + 0.05f) return nullptr;

    ComPtr<ID2D1PathGeometry> geo;
    if (FAILED(factory->CreatePathGeometry(&geo))) return nullptr;
    ComPtr<ID2D1GeometrySink> sink;
    if (FAILED(geo->Open(&sink))) return nullptr;
    sink->SetFillMode(D2D1_FILL_MODE_WINDING);

    float cs = std::cos(start_rad), ss = std::sin(start_rad);
    float ce = std::cos(end_rad), se = std::sin(end_rad);
    D2D1_ARC_SIZE arc_sz = (sweep > kPi) ? D2D1_ARC_SIZE_LARGE : D2D1_ARC_SIZE_SMALL;

    sink->BeginFigure(D2D1::Point2F(outer_r * cs, outer_r * ss), D2D1_FIGURE_BEGIN_FILLED);
    sink->AddArc(D2D1::ArcSegment(
        D2D1::Point2F(outer_r * ce, outer_r * se), D2D1::SizeF(outer_r, outer_r), 0.0f,
        D2D1_SWEEP_DIRECTION_CLOCKWISE, arc_sz));
    sink->AddLine(D2D1::Point2F(inner_r * ce, inner_r * se));
    sink->AddArc(D2D1::ArcSegment(
        D2D1::Point2F(inner_r * cs, inner_r * ss), D2D1::SizeF(inner_r, inner_r), 0.0f,
        D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, arc_sz));
    sink->EndFigure(D2D1_FIGURE_END_CLOSED);
    sink->Close();
    return geo;
}

ComPtr<ID2D1PathGeometry> make_polygon_ring_geometry(
    ID2D1Factory1* factory, const float* outer_xy, int outer_n, const float* inner_xy, int inner_n) {
    if (!outer_xy || outer_n < 3) return nullptr;
    ComPtr<ID2D1PathGeometry> geo;
    if (FAILED(factory->CreatePathGeometry(&geo))) return nullptr;
    ComPtr<ID2D1GeometrySink> sink;
    if (FAILED(geo->Open(&sink))) return nullptr;
    sink->SetFillMode(D2D1_FILL_MODE_ALTERNATE);

    sink->BeginFigure(D2D1::Point2F(outer_xy[0], outer_xy[1]), D2D1_FIGURE_BEGIN_FILLED);
    for (int i = 1; i < outer_n; ++i) {
        sink->AddLine(D2D1::Point2F(outer_xy[i * 2], outer_xy[i * 2 + 1]));
    }
    sink->EndFigure(D2D1_FIGURE_END_CLOSED);

    if (inner_xy && inner_n >= 3) {
        sink->BeginFigure(D2D1::Point2F(inner_xy[0], inner_xy[1]), D2D1_FIGURE_BEGIN_FILLED);
        for (int i = 1; i < inner_n; ++i) {
            sink->AddLine(D2D1::Point2F(inner_xy[i * 2], inner_xy[i * 2 + 1]));
        }
        sink->EndFigure(D2D1_FIGURE_END_CLOSED);
    }
    sink->Close();
    return geo;
}

ComPtr<ID2D1LinearGradientBrush> make_linear_brush(
    D2dCtx* ctx, float x0, float y0, float x1, float y1, const D2dGradStop* stops, int count) {
    if (!ctx || !ctx->d2d_ctx || !stops || count < 2) return nullptr;
    if (count <= 8) {
        float max_a = 0.0f;
        for (int i = 0; i < count; ++i) {
            max_a = std::max(max_a, std::clamp(stops[i].color.a, 0.0f, 1.0f));
        }
        if (max_a <= 1e-4f) return nullptr;
        float inv_max_a = 1.0f / max_a;

        auto q_pos = [](float v) -> uint16_t {
            return static_cast<uint16_t>(std::clamp(std::round(v * 10000.0f), 0.0f, 10000.0f));
        };
        auto q_col = [](float v) -> uint16_t {
            return static_cast<uint16_t>(std::clamp(std::round(v * 1024.0f), 0.0f, 1024.0f));
        };

        LinearBrushKey key{};
        key.count = static_cast<uint8_t>(count);
        for (int i = 0; i < count; ++i) {
            size_t base = static_cast<size_t>(i) * 5;
            key.data[base + 0] = q_pos(stops[i].pos);
            key.data[base + 1] = q_col(stops[i].color.r);
            key.data[base + 2] = q_col(stops[i].color.g);
            key.data[base + 3] = q_col(stops[i].color.b);
            key.data[base + 4] = q_col(stops[i].color.a * inv_max_a);
        }
        auto it = ctx->linear_brush_cache.find(key);
        if (it != ctx->linear_brush_cache.end() && it->second) {
            it->second->SetStartPoint(D2D1::Point2F(x0, y0));
            it->second->SetEndPoint(D2D1::Point2F(x1, y1));
            it->second->SetOpacity(max_a);
            return it->second;
        }
        if (ctx->linear_brush_cache.size() >= kMaxLinearBrushCacheEntries) {
            ctx->linear_brush_cache.clear();
        }
        std::array<D2D1_GRADIENT_STOP, 8> d2d_stops{};
        for (int i = 0; i < count; ++i) {
            size_t base = static_cast<size_t>(i) * 5;
            float pos = static_cast<float>(key.data[base + 0]) * (1.0f / 10000.0f);
            float r = static_cast<float>(key.data[base + 1]) * (1.0f / 1024.0f);
            float g = static_cast<float>(key.data[base + 2]) * (1.0f / 1024.0f);
            float b = static_cast<float>(key.data[base + 3]) * (1.0f / 1024.0f);
            float a = static_cast<float>(key.data[base + 4]) * (1.0f / 1024.0f);
            d2d_stops[static_cast<size_t>(i)] = D2D1::GradientStop(pos, D2D1::ColorF(r, g, b, a));
        }
        ComPtr<ID2D1GradientStopCollection> coll;
        if (FAILED(ctx->d2d_ctx->CreateGradientStopCollection(
                d2d_stops.data(), static_cast<UINT32>(count),
                D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP, &coll))) {
            return nullptr;
        }
        ComPtr<ID2D1LinearGradientBrush> brush;
        if (FAILED(ctx->d2d_ctx->CreateLinearGradientBrush(
                D2D1::LinearGradientBrushProperties(D2D1::Point2F(x0, y0), D2D1::Point2F(x1, y1)),
                coll.Get(), &brush))) {
            return nullptr;
        }
        brush->SetOpacity(max_a);
        ctx->linear_brush_cache.emplace(key, brush);
        return brush;
    }

    D2D1_GRADIENT_STOP stack_stops[16];
    std::vector<D2D1_GRADIENT_STOP> heap_stops;
    D2D1_GRADIENT_STOP* d2d_stops = stack_stops;
    if (count > 16) {
        heap_stops.resize(static_cast<size_t>(count));
        d2d_stops = heap_stops.data();
    }
    for (int i = 0; i < count; ++i) {
        d2d_stops[i] = D2D1::GradientStop(stops[i].pos, to_d2d(stops[i].color));
    }
    ComPtr<ID2D1GradientStopCollection> coll;
    if (FAILED(ctx->d2d_ctx->CreateGradientStopCollection(
            d2d_stops, static_cast<UINT32>(count),
            D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP, &coll))) {
        return nullptr;
    }
    ComPtr<ID2D1LinearGradientBrush> brush;
    if (FAILED(ctx->d2d_ctx->CreateLinearGradientBrush(
            D2D1::LinearGradientBrushProperties(D2D1::Point2F(x0, y0), D2D1::Point2F(x1, y1)),
            coll.Get(), &brush))) {
        return nullptr;
    }
    return brush;
}

bool prepare_backdrop_brush(D2dCtx* ctx, float opacity) {
    if (!ctx->has_backdrop || !ctx->backdrop_brush || opacity <= 0.002f) return false;
    D2D1_MATRIX_3X2_F cur;
    ctx->d2d_ctx->GetTransform(&cur);
    D2D1::Matrix3x2F total = *D2D1::Matrix3x2F::ReinterpretBaseType(&cur) *
                             D2D1::Matrix3x2F::Scale(ctx->logical_scale, ctx->logical_scale) *
                             D2D1::Matrix3x2F::Translation(ctx->backdrop_origin_x, ctx->backdrop_origin_y);
    if (!total.Invert()) return false;
    ctx->backdrop_brush->SetTransform(total);
    ctx->backdrop_brush->SetOpacity(std::clamp(opacity, 0.0f, 1.0f));
    return true;
}

void measure_string_span(
    D2dCtx* ctx, const wchar_t* ptr, size_t len, float size, bool bold, float max_w, int max_lines, float* out_w, float* out_h) {
    if (!ctx || !ptr || len == 0 || !ctx->dwrite) {
        if (out_w) *out_w = 0.0f;
        if (out_h) *out_h = 0.0f;
        return;
    }
    uint32_t size_q = static_cast<uint32_t>(std::clamp(std::round(size * 10.0f), 50.0f, 1600.0f));
    float layout_w = (max_w > 0.0f) ? max_w : 4096.0f;
    uint32_t w_half = static_cast<uint32_t>(std::clamp(std::round(layout_w * 2.0f), 2.0f, 32768.0f));
    int32_t lines_q = std::clamp(max_lines, 1, 16);

    auto& probe = ctx->measure_probe_key;
    probe.text.assign(ptr, len);
    probe.size_tenths = size_q;
    probe.max_w_half = w_half;
    probe.max_lines = (bold ? lines_q : -lines_q);

    auto it = ctx->measure_cache.find(probe);
    if (it != ctx->measure_cache.end()) {
        if (out_w) *out_w = it->second.w;
        if (out_h) *out_h = it->second.h;
        return;
    }

    bool wrap = (lines_q > 1 && max_w > 0.0f);
    IDWriteTextFormat* fmt = get_text_format(ctx, size, bold, 0, wrap);
    if (!fmt) {
        if (out_w) *out_w = 0.0f;
        if (out_h) *out_h = 0.0f;
        return;
    }
    ComPtr<IDWriteTextLayout> layout;
    float layout_h = (lines_q > 1) ? (size * 1.45f * static_cast<float>(lines_q)) : 256.0f;
    if (!create_tabular_text_layout(
            ctx, probe.text.c_str(), static_cast<UINT32>(len), fmt, static_cast<float>(w_half) * 0.5f, layout_h, layout)) {
        if (out_w) *out_w = 0.0f;
        if (out_h) *out_h = 0.0f;
        return;
    }
    DWRITE_TEXT_METRICS m{};
    layout->GetMetrics(&m);
    MeasureVal val{m.widthIncludingTrailingWhitespace, m.height};
    if (ctx->measure_cache.size() >= kMaxMeasureCacheEntries) {
        ctx->measure_cache.clear();
    }
    ctx->measure_cache.emplace(probe, val);
    if (out_w) *out_w = val.w;
    if (out_h) *out_h = val.h;
}

void measure_string_raw(
    D2dCtx* ctx, const std::wstring& s, float size, bool bold, float max_w, int max_lines, float* out_w, float* out_h) {
    measure_string_span(ctx, s.c_str(), s.size(), size, bold, max_w, max_lines, out_w, out_h);
}

std::wstring ellipsize_start_or_middle(
    D2dCtx* ctx, const std::wstring& s, float size, bool bold, float max_w, int mode) {
    if (s.empty() || max_w <= 4.0f) return s;
    float w = 0.0f;
    measure_string_raw(ctx, s, size, bold, 0.0f, 1, &w, nullptr);
    if (w <= max_w) return s;

    auto is_high_surrogate = [](wchar_t c) { return c >= 0xD800 && c <= 0xDBFF; };
    auto is_low_surrogate = [](wchar_t c) { return c >= 0xDC00 && c <= 0xDFFF; };

    const std::wstring ell = L"\u2026";
    if (mode == 1) {
        // Ellipsize start: keep suffix without splitting a UTF-16 surrogate pair
        size_t lo = 1, hi = s.size();
        std::wstring best = ell;
        while (lo <= hi) {
            size_t mid = (lo + hi) / 2;
            size_t start = s.size() - mid;
            if (start < s.size() && is_low_surrogate(s[start])) {
                ++start;
            }
            std::wstring cand = ell + s.substr(start);
            float cw = 0.0f;
            measure_string_raw(ctx, cand, size, bold, 0.0f, 1, &cw, nullptr);
            if (cw <= max_w) {
                best = std::move(cand);
                lo = mid + 1;
            } else {
                if (mid == 0) break;
                hi = mid - 1;
            }
        }
        return best;
    }
    // Ellipsize middle: keep head + ellipsis + tail without splitting UTF-16 surrogate pairs
    size_t lo = 2, hi = s.size();
    std::wstring best = ell;
    while (lo <= hi) {
        size_t mid = (lo + hi) / 2;
        size_t head = (mid + 1) / 2;
        size_t tail = mid - head;
        if (head > 0 && is_high_surrogate(s[head - 1])) {
            --head;
        }
        size_t tail_start = s.size() - tail;
        if (tail_start < s.size() && is_low_surrogate(s[tail_start])) {
            ++tail_start;
        }
        if (tail_start < head) {
            tail_start = head;
        }
        std::wstring cand = s.substr(0, head) + ell + s.substr(tail_start);
        float cw = 0.0f;
        measure_string_raw(ctx, cand, size, bold, 0.0f, 1, &cw, nullptr);
        if (cw <= max_w) {
            best = std::move(cand);
            lo = mid + 1;
        } else {
            if (mid == 0) break;
            hi = mid - 1;
        }
    }
    return best;
}

bool ensure_wic(D2dCtx* ctx) {
    if (!ctx) return false;
    if (ctx->wic) return true;
    CoCreateInstance(
        CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER,
        IID_PPV_ARGS(&ctx->wic));
    return ctx->wic != nullptr;
}

int register_wic_bitmap(D2dCtx* ctx, IWICBitmapSource* src, int max_dim) {
    if (!ensure_wic(ctx) || !ctx->d2d_ctx || !src) return 0;
    UINT w = 0, h = 0;
    if (FAILED(src->GetSize(&w, &h)) || w == 0 || h == 0) return 0;

    ComPtr<IWICBitmapSource> current = src;
    if (max_dim > 0 && (static_cast<int>(w) > max_dim || static_cast<int>(h) > max_dim)) {
        double scale = static_cast<double>(max_dim) / static_cast<double>(std::max(w, h));
        UINT nw = std::max<UINT>(1, static_cast<UINT>(std::round(w * scale)));
        UINT nh = std::max<UINT>(1, static_cast<UINT>(std::round(h * scale)));
        ComPtr<IWICBitmapScaler> scaler;
        if (SUCCEEDED(ctx->wic->CreateBitmapScaler(&scaler)) &&
            SUCCEEDED(scaler->Initialize(current.Get(), nw, nh, WICBitmapInterpolationModeFant))) {
            current = scaler;
        }
    }

    ComPtr<IWICFormatConverter> conv;
    if (FAILED(ctx->wic->CreateFormatConverter(&conv))) return 0;
    if (FAILED(conv->Initialize(
            current.Get(),
            GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            nullptr,
            0.0,
            WICBitmapPaletteTypeMedianCut))) {
        return 0;
    }

    ComPtr<ID2D1Bitmap1> bmp;
    D2D1_BITMAP_PROPERTIES1 props = D2D1::BitmapProperties1(
        D2D1_BITMAP_OPTIONS_NONE,
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
    if (FAILED(ctx->d2d_ctx->CreateBitmapFromWicBitmap(conv.Get(), &props, &bmp))) {
        return 0;
    }

    int id = ctx->next_bmp_id++;
    ctx->bitmaps.emplace(id, std::move(bmp));
    return id;
}

void release_device_resources(D2dCtx* ctx) {
    if (!ctx) return;
    ctx->transform_stack.clear();
    ctx->layout_map.clear();
    ctx->layout_lru.clear();
    ctx->measure_cache.clear();
    ctx->format_cache.clear();
    ctx->tabular_typography.Reset();
    ctx->linear_brush_cache.clear();
    ctx->annulus_cache.clear();
    ctx->rrect_hole_cache.clear();
    ctx->shadow_cache.clear();
    ctx->sheen_cache.clear();
    ctx->arc_cache.clear();
    ctx->path_cache.clear();
    ctx->bitmaps.clear();

    ctx->has_backdrop = false;
    ctx->backdrop_w = 0;
    ctx->backdrop_h = 0;
    ctx->last_blur_sigma = -1.0f;
    ctx->last_blur_sat = -1.0f;
    ctx->backdrop_brush.Reset();
    ctx->blurred_backdrop.Reset();
    ctx->raw_backdrop.Reset();

    ctx->solid_brush.Reset();
    ctx->ghost_brush.Reset();
    ctx->round_stroke.Reset();
    ctx->flat_stroke.Reset();

    if (ctx->d2d_ctx) {
        ctx->d2d_ctx->SetTarget(nullptr);
    }
    ctx->target_bmp.Reset();

    if (ctx->dcomp_target) {
        ctx->dcomp_target->SetRoot(nullptr);
    }
    if (ctx->dcomp_visual) {
        ctx->dcomp_visual->SetContent(nullptr);
    }
    if (ctx->dcomp_dev) {
        ctx->dcomp_dev->Commit();
    }
    ctx->dcomp_visual.Reset();
    ctx->dcomp_target.Reset();
    ctx->dcomp_dev.Reset();

    ctx->swapchain.Reset();
    ctx->d2d_ctx.Reset();
    ctx->d2d_dev.Reset();
    ctx->d2d_factory.Reset();
    ctx->dxgi_dev.Reset();
    ctx->d3d_dev.Reset();
}

bool init_device_resources(D2dCtx* ctx) {
    UINT flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
    D3D_FEATURE_LEVEL levels[] = {
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    };
    HRESULT hr = D3D11CreateDevice(
        nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, flags,
        levels, ARRAYSIZE(levels), D3D11_SDK_VERSION,
        &ctx->d3d_dev, nullptr, nullptr);
    if (FAILED(hr)) {
        hr = D3D11CreateDevice(
            nullptr, D3D_DRIVER_TYPE_WARP, nullptr, flags,
            levels, ARRAYSIZE(levels), D3D11_SDK_VERSION,
            &ctx->d3d_dev, nullptr, nullptr);
    }
    if (FAILED(hr)) return false;

    if (FAILED(ctx->d3d_dev.As(&ctx->dxgi_dev))) return false;

    ComPtr<IDXGIFactory2> dxgi_factory;
    hr = CreateDXGIFactory2(0, IID_PPV_ARGS(&dxgi_factory));
    if (FAILED(hr)) return false;

    DXGI_SWAP_CHAIN_DESC1 sc_desc{};
    sc_desc.Width = static_cast<UINT>(ctx->phys_w);
    sc_desc.Height = static_cast<UINT>(ctx->phys_h);
    sc_desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    sc_desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sc_desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    sc_desc.BufferCount = 3;
    sc_desc.SampleDesc.Count = 1;
    sc_desc.AlphaMode = DXGI_ALPHA_MODE_PREMULTIPLIED;
    sc_desc.Scaling = DXGI_SCALING_STRETCH;

    hr = dxgi_factory->CreateSwapChainForComposition(
        ctx->dxgi_dev.Get(), &sc_desc, nullptr, &ctx->swapchain);
    if (FAILED(hr)) {
        sc_desc.BufferCount = 2;
        hr = dxgi_factory->CreateSwapChainForComposition(
            ctx->dxgi_dev.Get(), &sc_desc, nullptr, &ctx->swapchain);
    }
    if (FAILED(hr)) {
        sc_desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL;
        hr = dxgi_factory->CreateSwapChainForComposition(
            ctx->dxgi_dev.Get(), &sc_desc, nullptr, &ctx->swapchain);
    }
    if (FAILED(hr)) return false;

    D2D1_FACTORY_OPTIONS fopts{};
    hr = D2D1CreateFactory(
        D2D1_FACTORY_TYPE_SINGLE_THREADED,
        __uuidof(ID2D1Factory1),
        &fopts,
        reinterpret_cast<void**>(ctx->d2d_factory.GetAddressOf()));
    if (FAILED(hr)) return false;

    hr = ctx->d2d_factory->CreateDevice(ctx->dxgi_dev.Get(), &ctx->d2d_dev);
    if (FAILED(hr)) return false;

    hr = ctx->d2d_dev->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE, &ctx->d2d_ctx);
    if (FAILED(hr)) return false;

    ctx->d2d_ctx->SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    ctx->d2d_ctx->SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
    ctx->d2d_ctx->SetUnitMode(D2D1_UNIT_MODE_DIPS);

    if (!bind_swapchain_target(ctx)) return false;

    hr = DCompositionCreateDevice(ctx->dxgi_dev.Get(), IID_PPV_ARGS(&ctx->dcomp_dev));
    if (FAILED(hr)) return false;
    hr = ctx->dcomp_dev->CreateTargetForHwnd(ctx->hwnd, TRUE, &ctx->dcomp_target);
    if (FAILED(hr)) return false;
    hr = ctx->dcomp_dev->CreateVisual(&ctx->dcomp_visual);
    if (FAILED(hr)) return false;
    hr = ctx->dcomp_visual->SetContent(ctx->swapchain.Get());
    if (FAILED(hr)) return false;
    hr = ctx->dcomp_target->SetRoot(ctx->dcomp_visual.Get());
    if (FAILED(hr)) return false;
    ctx->dcomp_dev->Commit();

    ctx->d2d_ctx->CreateSolidColorBrush(D2D1::ColorF(1, 1, 1, 1), &ctx->solid_brush);
    ctx->d2d_ctx->CreateSolidColorBrush(D2D1::ColorF(1, 1, 1, 1), &ctx->ghost_brush);

    D2D1_STROKE_STYLE_PROPERTIES round_props = D2D1::StrokeStyleProperties(
        D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_ROUND, D2D1_LINE_JOIN_ROUND);
    ctx->d2d_factory->CreateStrokeStyle(&round_props, nullptr, 0, &ctx->round_stroke);

    D2D1_STROKE_STYLE_PROPERTIES flat_props = D2D1::StrokeStyleProperties(
        D2D1_CAP_STYLE_FLAT, D2D1_CAP_STYLE_FLAT, D2D1_CAP_STYLE_FLAT, D2D1_LINE_JOIN_MITER);
    ctx->d2d_factory->CreateStrokeStyle(&flat_props, nullptr, 0, &ctx->flat_stroke);

    return ctx->solid_brush != nullptr;
}

bool rebuild_blurred_backdrop(D2dCtx* ctx, float blur_sigma_px, float saturation) {
    if (!ctx || !ctx->d2d_ctx || !ctx->raw_backdrop || ctx->backdrop_w <= 0 || ctx->backdrop_h <= 0) {
        return false;
    }
    float sigma = std::clamp(blur_sigma_px, 0.05f, 64.0f);
    float sat = std::clamp(saturation, 0.5f, 2.0f);
    if (ctx->has_backdrop && ctx->backdrop_brush &&
        std::fabs(sigma - ctx->last_blur_sigma) < 0.01f &&
        std::fabs(sat - ctx->last_blur_sat) < 0.005f) {
        return true;
    }

    if (!ctx->blurred_backdrop) {
        D2D1_BITMAP_PROPERTIES1 dst_props = D2D1::BitmapProperties1(
            D2D1_BITMAP_OPTIONS_TARGET,
            D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED),
            96.0f, 96.0f);
        HRESULT hr = ctx->d2d_ctx->CreateBitmap(
            D2D1::SizeU(static_cast<UINT32>(ctx->backdrop_w), static_cast<UINT32>(ctx->backdrop_h)),
            nullptr, 0, &dst_props, &ctx->blurred_backdrop);
        if (FAILED(hr) || !ctx->blurred_backdrop) {
            return false;
        }
        ctx->backdrop_brush.Reset();
    }

    ComPtr<ID2D1Effect> blur_fx;
    HRESULT hr = ctx->d2d_ctx->CreateEffect(CLSID_D2D1GaussianBlur, &blur_fx);
    if (FAILED(hr) || !blur_fx) {
        return false;
    }
    blur_fx->SetInput(0, ctx->raw_backdrop.Get());
    blur_fx->SetValue(D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION, sigma);
    blur_fx->SetValue(D2D1_GAUSSIANBLUR_PROP_BORDER_MODE, D2D1_BORDER_MODE_HARD);
    // Use QUALITY for small sigmas so 1%-12% blur steps linearly without coarse downsampling jumps.
    D2D1_GAUSSIANBLUR_OPTIMIZATION opt =
        (sigma < 4.0f) ? D2D1_GAUSSIANBLUR_OPTIMIZATION_QUALITY : D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED;
    blur_fx->SetValue(D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION, opt);

    ComPtr<ID2D1Effect> final_fx = blur_fx;
    ComPtr<ID2D1Effect> sat_fx;
    if (std::fabs(sat - 1.0f) > 0.002f &&
        SUCCEEDED(ctx->d2d_ctx->CreateEffect(CLSID_D2D1Saturation, &sat_fx)) && sat_fx) {
        sat_fx->SetInputEffect(0, blur_fx.Get());
        sat_fx->SetValue(D2D1_SATURATION_PROP_SATURATION, sat);
        final_fx = sat_fx;
    }

    ComPtr<ID2D1Image> prev_target;
    ctx->d2d_ctx->GetTarget(&prev_target);
    float prev_dx = 96.0f, prev_dy = 96.0f;
    ctx->d2d_ctx->GetDpi(&prev_dx, &prev_dy);

    ctx->d2d_ctx->SetTarget(ctx->blurred_backdrop.Get());
    ctx->d2d_ctx->SetDpi(96.0f, 96.0f);
    ctx->d2d_ctx->BeginDraw();
    ctx->d2d_ctx->SetTransform(D2D1::Matrix3x2F::Identity());
    ctx->d2d_ctx->Clear(D2D1::ColorF(0, 0, 0, 1));
    ctx->d2d_ctx->DrawImage(final_fx.Get(), D2D1_INTERPOLATION_MODE_LINEAR);
    hr = ctx->d2d_ctx->EndDraw();

    ctx->d2d_ctx->SetTarget(prev_target.Get());
    ctx->d2d_ctx->SetDpi(prev_dx, prev_dy);

    if (FAILED(hr)) {
        return false;
    }

    if (!ctx->backdrop_brush) {
        D2D1_BITMAP_BRUSH_PROPERTIES1 bprops = D2D1::BitmapBrushProperties1(
            D2D1_EXTEND_MODE_CLAMP, D2D1_EXTEND_MODE_CLAMP, D2D1_INTERPOLATION_MODE_LINEAR);
        hr = ctx->d2d_ctx->CreateBitmapBrush(ctx->blurred_backdrop.Get(), &bprops, nullptr, &ctx->backdrop_brush);
        if (FAILED(hr) || !ctx->backdrop_brush) {
            return false;
        }
    }

    ctx->has_backdrop = true;
    ctx->last_blur_sigma = sigma;
    ctx->last_blur_sat = sat;
    return true;
}

} // namespace

extern "C" {

void d2d_destroy(D2dCtx* ctx);

D2dCtx* d2d_create(HWND hwnd, int phys_w, int phys_h, float logical_scale) {
    auto* ctx = new D2dCtx();
    ctx->hwnd = hwnd;
    ctx->phys_w = std::max(1, phys_w);
    ctx->phys_h = std::max(1, phys_h);
    ctx->logical_scale = std::clamp(logical_scale, 0.5f, 4.0f);

    HRESULT hr_com = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (SUCCEEDED(hr_com)) {
        ctx->com_inited = true;
    }

    DWriteCreateFactory(
        DWRITE_FACTORY_TYPE_SHARED,
        __uuidof(IDWriteFactory),
        reinterpret_cast<IUnknown**>(ctx->dwrite.GetAddressOf()));
    pick_best_font(ctx);

    if (!init_device_resources(ctx)) {
        d2d_destroy(ctx);
        return nullptr;
    }
    return ctx;
}

int d2d_recreate(D2dCtx* ctx) {
    if (!ctx) return 0;
    release_device_resources(ctx);
    return init_device_resources(ctx) ? 1 : 0;
}

void d2d_set_font_family(D2dCtx* ctx, const uint16_t* family, int len) {
    if (!ctx || !ctx->dwrite) return;
    std::wstring next_family;
    if (family && len > 0) {
        std::wstring requested(reinterpret_cast<const wchar_t*>(family), static_cast<size_t>(len));
        ComPtr<IDWriteFontCollection> coll;
        UINT32 idx = 0;
        BOOL exists = FALSE;
        if (SUCCEEDED(ctx->dwrite->GetSystemFontCollection(&coll)) && coll &&
            SUCCEEDED(coll->FindFamilyName(requested.c_str(), &idx, &exists)) && exists) {
            next_family = std::move(requested);
        }
    }
    if (next_family.empty()) {
        std::wstring prev = ctx->font_family;
        ctx->font_family = L"Segoe UI";
        pick_best_font(ctx);
        if (ctx->font_family != prev) {
            ctx->format_cache.clear();
            ctx->layout_map.clear();
            ctx->layout_lru.clear();
            ctx->measure_cache.clear();
        }
        return;
    }
    if (next_family != ctx->font_family) {
        ctx->font_family = std::move(next_family);
        ctx->format_cache.clear();
        ctx->layout_map.clear();
        ctx->layout_lru.clear();
        ctx->measure_cache.clear();
    }
}

void d2d_destroy(D2dCtx* ctx) {
    if (!ctx) return;
    release_device_resources(ctx);
    bool com = ctx->com_inited;
    delete ctx;
    if (com) {
        CoUninitialize();
    }
}

int d2d_capture_backdrop(D2dCtx* ctx, int screen_x, int screen_y, int phys_w, int phys_h, float blur_sigma_px) {
    if (!ctx || !ctx->d2d_ctx || phys_w <= 0 || phys_h <= 0) return 0;

    // When recapturing while visible, temporarily exclude our own HWND from screen capture
    // via WDA_EXCLUDEFROMCAPTURE + one DwmFlush so BitBlt sees the clean desktop/windows
    // underneath with zero visual flicker on screen, then immediately restore normal capture.
    constexpr DWORD kWdaNone = 0x00000000;
    constexpr DWORD kWdaExcludeFromCapture = 0x00000011;
    bool excluded_self = false;
    if (ctx->hwnd && IsWindowVisible(ctx->hwnd)) {
        if (SetWindowDisplayAffinity(ctx->hwnd, kWdaExcludeFromCapture)) {
            excluded_self = true;
            DwmFlush();
        }
    }

    HDC screen_dc = GetDC(nullptr);
    if (!screen_dc) {
        if (excluded_self) SetWindowDisplayAffinity(ctx->hwnd, kWdaNone);
        return 0;
    }

    HDC mem_dc = CreateCompatibleDC(screen_dc);
    if (!mem_dc) {
        ReleaseDC(nullptr, screen_dc);
        if (excluded_self) SetWindowDisplayAffinity(ctx->hwnd, kWdaNone);
        return 0;
    }

    BITMAPINFO bmi{};
    bmi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bmi.bmiHeader.biWidth = phys_w;
    bmi.bmiHeader.biHeight = -phys_h; // top-down
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    void* bits = nullptr;
    HBITMAP dib = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &bits, nullptr, 0);
    if (!dib || !bits) {
        if (dib) DeleteObject(dib);
        DeleteDC(mem_dc);
        ReleaseDC(nullptr, screen_dc);
        if (excluded_self) SetWindowDisplayAffinity(ctx->hwnd, kWdaNone);
        return 0;
    }

    HGDIOBJ old = SelectObject(mem_dc, dib);
    BOOL ok = BitBlt(mem_dc, 0, 0, phys_w, phys_h, screen_dc, screen_x, screen_y, SRCCOPY | CAPTUREBLT);
    SelectObject(mem_dc, old);
    DeleteDC(mem_dc);
    ReleaseDC(nullptr, screen_dc);

    if (excluded_self) {
        SetWindowDisplayAffinity(ctx->hwnd, kWdaNone);
    }

    if (!ok) {
        DeleteObject(dib);
        return 0;
    }

    // Reuse existing GPU raw_backdrop when monitor dimensions match to avoid texture churn.
    HRESULT hr = S_OK;
    if (ctx->raw_backdrop && ctx->backdrop_w == phys_w && ctx->backdrop_h == phys_h) {
        hr = ctx->raw_backdrop->CopyFromMemory(nullptr, bits, static_cast<UINT32>(phys_w * 4));
    } else {
        ctx->has_backdrop = false;
        ctx->backdrop_brush.Reset();
        ctx->blurred_backdrop.Reset();
        ctx->raw_backdrop.Reset();
        // D2D1_ALPHA_MODE_IGNORE treats GDI BitBlt RGBX pixels as opaque without a CPU pixel loop.
        ComPtr<ID2D1Bitmap1> src_bmp;
        D2D1_BITMAP_PROPERTIES1 src_props = D2D1::BitmapProperties1(
            D2D1_BITMAP_OPTIONS_NONE,
            D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_IGNORE),
            96.0f, 96.0f);
        hr = ctx->d2d_ctx->CreateBitmap(
            D2D1::SizeU(static_cast<UINT32>(phys_w), static_cast<UINT32>(phys_h)),
            bits, static_cast<UINT32>(phys_w * 4), &src_props, &src_bmp);
        if (SUCCEEDED(hr) && src_bmp) {
            ctx->raw_backdrop = std::move(src_bmp);
            ctx->backdrop_w = phys_w;
            ctx->backdrop_h = phys_h;
        }
    }
    DeleteObject(dib);
    if (FAILED(hr) || !ctx->raw_backdrop) return 0;

    ctx->last_blur_sigma = -1.0f;
    ctx->last_blur_sat = -1.0f;

    if (blur_sigma_px <= 0.01f) {
        ctx->has_backdrop = false;
        ctx->last_blur_sigma = 0.0f;
        ctx->last_blur_sat = 1.0f;
        return 1;
    }
    float def_sat = 1.0f + 0.24f * std::clamp(blur_sigma_px / (32.0f * ctx->logical_scale), 0.0f, 1.0f);
    return rebuild_blurred_backdrop(ctx, blur_sigma_px, def_sat) ? 1 : 0;
}

int d2d_set_backdrop_blur(D2dCtx* ctx, float blur_sigma_px, float saturation) {
    if (!ctx || !ctx->raw_backdrop) return 0;
    if (blur_sigma_px <= 0.01f) {
        ctx->has_backdrop = false;
        ctx->last_blur_sigma = 0.0f;
        ctx->last_blur_sat = 1.0f;
        return 1;
    }
    return rebuild_blurred_backdrop(ctx, blur_sigma_px, saturation) ? 1 : 0;
}

void d2d_set_backdrop_origin(D2dCtx* ctx, float origin_x_px, float origin_y_px) {
    if (!ctx) return;
    ctx->backdrop_origin_x = origin_x_px;
    ctx->backdrop_origin_y = origin_y_px;
}

int d2d_has_backdrop(D2dCtx* ctx) {
    return (ctx && ctx->has_backdrop) ? 1 : 0;
}

int d2d_has_raw_backdrop(D2dCtx* ctx) {
    return (ctx && ctx->raw_backdrop != nullptr) ? 1 : 0;
}

void d2d_resize(D2dCtx* ctx, int phys_w, int phys_h, float logical_scale) {
    if (!ctx || !ctx->swapchain || !ctx->d2d_ctx) return;
    int nw = std::max(1, phys_w);
    int nh = std::max(1, phys_h);
    ctx->logical_scale = std::clamp(logical_scale, 0.5f, 4.0f);
    if (nw != ctx->phys_w || nh != ctx->phys_h || !ctx->target_bmp) {
        ctx->phys_w = nw;
        ctx->phys_h = nh;
        ctx->d2d_ctx->SetTarget(nullptr);
        ctx->target_bmp.Reset();
        HRESULT hr = ctx->swapchain->ResizeBuffers(
            0, static_cast<UINT>(nw), static_cast<UINT>(nh), DXGI_FORMAT_B8G8R8A8_UNORM, 0);
        if (SUCCEEDED(hr)) {
            bind_swapchain_target(ctx);
        }
    }
}

void d2d_begin_frame(D2dCtx* ctx, float logical_scale) {
    if (!ctx || !ctx->d2d_ctx || !ctx->target_bmp) return;
    ctx->logical_scale = std::clamp(logical_scale, 0.5f, 4.0f);
    ctx->d2d_ctx->SetTarget(ctx->target_bmp.Get());
    ctx->d2d_ctx->SetDpi(96.0f * ctx->logical_scale, 96.0f * ctx->logical_scale);
    ctx->transform_stack.clear();
    ctx->d2d_ctx->BeginDraw();
    ctx->d2d_ctx->SetTransform(D2D1::Matrix3x2F::Identity());
    ctx->d2d_ctx->Clear(D2D1::ColorF(0.0f, 0.0f, 0.0f, 0.0f));
}

int d2d_end_frame(D2dCtx* ctx, int sync_interval) {
    if (!ctx || !ctx->d2d_ctx || !ctx->swapchain || !ctx->target_bmp) return 0;
    HRESULT hr = ctx->d2d_ctx->EndDraw();
    if (FAILED(hr)) {
        return 0;
    }
    hr = ctx->swapchain->Present(static_cast<UINT>(sync_interval), 0);
    if (FAILED(hr)) {
        return 0;
    }
    if (ctx->dcomp_dev) {
        ctx->dcomp_dev->Commit();
    }
    return 1;
}

void d2d_save(D2dCtx* ctx) {
    if (!ctx || !ctx->d2d_ctx) return;
    D2D1_MATRIX_3X2_F m;
    ctx->d2d_ctx->GetTransform(&m);
    ctx->transform_stack.push_back(m);
}

void d2d_restore(D2dCtx* ctx) {
    if (!ctx || !ctx->d2d_ctx || ctx->transform_stack.empty()) return;
    D2D1_MATRIX_3X2_F m = ctx->transform_stack.back();
    ctx->transform_stack.pop_back();
    ctx->d2d_ctx->SetTransform(m);
}

void d2d_translate(D2dCtx* ctx, float tx, float ty) {
    if (!ctx || !ctx->d2d_ctx) return;
    D2D1_MATRIX_3X2_F cur;
    ctx->d2d_ctx->GetTransform(&cur);
    D2D1::Matrix3x2F next = D2D1::Matrix3x2F::Translation(tx, ty) * (*D2D1::Matrix3x2F::ReinterpretBaseType(&cur));
    ctx->d2d_ctx->SetTransform(next);
}

void d2d_scale(D2dCtx* ctx, float sx, float sy) {
    if (!ctx || !ctx->d2d_ctx) return;
    D2D1_MATRIX_3X2_F cur;
    ctx->d2d_ctx->GetTransform(&cur);
    D2D1::Matrix3x2F next = D2D1::Matrix3x2F::Scale(sx, sy) * (*D2D1::Matrix3x2F::ReinterpretBaseType(&cur));
    ctx->d2d_ctx->SetTransform(next);
}

void d2d_rotate(D2dCtx* ctx, float radians) {
    if (!ctx || !ctx->d2d_ctx) return;
    D2D1_MATRIX_3X2_F cur;
    ctx->d2d_ctx->GetTransform(&cur);
    float deg = radians * (180.0f / kPi);
    D2D1::Matrix3x2F next = D2D1::Matrix3x2F::Rotation(deg) * (*D2D1::Matrix3x2F::ReinterpretBaseType(&cur));
    ctx->d2d_ctx->SetTransform(next);
}

void d2d_concat_matrix(D2dCtx* ctx, float m11, float m12, float m21, float m22, float dx, float dy) {
    if (!ctx || !ctx->d2d_ctx) return;
    D2D1_MATRIX_3X2_F cur;
    ctx->d2d_ctx->GetTransform(&cur);
    D2D1::Matrix3x2F m(m11, m12, m21, m22, dx, dy);
    D2D1::Matrix3x2F next = m * (*D2D1::Matrix3x2F::ReinterpretBaseType(&cur));
    ctx->d2d_ctx->SetTransform(next);
}

void d2d_push_clip_rect(D2dCtx* ctx, float x, float y, float w, float h) {
    if (!ctx || !ctx->d2d_ctx) return;
    ctx->d2d_ctx->PushAxisAlignedClip(D2D1::RectF(x, y, x + w, y + h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
}

void d2d_pop_clip_rect(D2dCtx* ctx) {
    if (!ctx || !ctx->d2d_ctx) return;
    ctx->d2d_ctx->PopAxisAlignedClip();
}

void d2d_push_clip_rounded_rect_hole(D2dCtx* ctx, float x, float y, float w, float h, float r, float hole_r) {
    if (!ctx || !ctx->d2d_ctx) return;
    ID2D1PathGeometry* geo = get_rounded_rect_hole_geometry(ctx, x, y, w, h, r, hole_r);
    D2D1_LAYER_PARAMETERS1 lp = D2D1::LayerParameters1();
    lp.contentBounds = D2D1::RectF(x - 1.0f, y - 1.0f, x + w + 1.0f, y + h + 1.0f);
    lp.geometricMask = geo;
    lp.maskAntialiasMode = D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
    lp.maskTransform = D2D1::IdentityMatrix();
    lp.opacity = 1.0f;
    ctx->d2d_ctx->PushLayer(&lp, nullptr);
}

void d2d_push_clip_sector(D2dCtx* ctx, float inner_r, float outer_r, float start_rad, float end_rad) {
    if (!ctx || !ctx->d2d_ctx) return;
    auto geo = make_sector_geometry(ctx->d2d_factory.Get(), inner_r, outer_r, start_rad, end_rad);
    float pad = outer_r + 2.0f;
    D2D1_LAYER_PARAMETERS1 lp = D2D1::LayerParameters1();
    lp.contentBounds = D2D1::RectF(-pad, -pad, pad, pad);
    lp.geometricMask = geo.Get();
    lp.maskAntialiasMode = D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
    lp.maskTransform = D2D1::IdentityMatrix();
    lp.opacity = 1.0f;
    ctx->d2d_ctx->PushLayer(&lp, nullptr);
}

void d2d_push_clip_polygon_ring(
    D2dCtx* ctx, const float* outer_xy, int outer_n, const float* inner_xy, int inner_n) {
    if (!ctx || !ctx->d2d_ctx) return;
    auto geo = make_polygon_ring_geometry(ctx->d2d_factory.Get(), outer_xy, outer_n, inner_xy, inner_n);
    D2D1_RECT_F bounds = D2D1::InfiniteRect();
    if (outer_xy && outer_n >= 3) {
        float min_x = outer_xy[0], max_x = outer_xy[0];
        float min_y = outer_xy[1], max_y = outer_xy[1];
        for (int i = 1; i < outer_n; ++i) {
            min_x = std::min(min_x, outer_xy[i * 2]);
            max_x = std::max(max_x, outer_xy[i * 2]);
            min_y = std::min(min_y, outer_xy[i * 2 + 1]);
            max_y = std::max(max_y, outer_xy[i * 2 + 1]);
        }
        bounds = D2D1::RectF(min_x - 2.0f, min_y - 2.0f, max_x + 2.0f, max_y + 2.0f);
    }
    D2D1_LAYER_PARAMETERS1 lp = D2D1::LayerParameters1();
    lp.contentBounds = bounds;
    lp.geometricMask = geo.Get();
    lp.maskAntialiasMode = D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
    lp.maskTransform = D2D1::IdentityMatrix();
    lp.opacity = 1.0f;
    ctx->d2d_ctx->PushLayer(&lp, nullptr);
}

void d2d_push_opacity_layer(D2dCtx* ctx, float x, float y, float w, float h, float opacity) {
    if (!ctx || !ctx->d2d_ctx) return;
    D2D1_LAYER_PARAMETERS1 lp = D2D1::LayerParameters1();
    lp.contentBounds = D2D1::RectF(x, y, x + w, y + h);
    lp.geometricMask = nullptr;
    lp.maskAntialiasMode = D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
    lp.maskTransform = D2D1::IdentityMatrix();
    lp.opacity = std::clamp(opacity, 0.0f, 1.0f);
    ctx->d2d_ctx->PushLayer(&lp, nullptr);
}

void d2d_pop_layer(D2dCtx* ctx) {
    if (!ctx || !ctx->d2d_ctx) return;
    ctx->d2d_ctx->PopLayer();
}

void d2d_fill_top_sheen(
    D2dCtx* ctx, float x, float y, float w, float h, float r, float sheen_h, D2dColor top_col) {
    if (!ctx || !ctx->d2d_ctx || w <= 0.1f || h <= 0.1f || top_col.a <= 0.002f) return;
    float sh = std::clamp(sheen_h, 0.5f, h);
    float rad = std::clamp(r, 0.0f, std::min(w, h) * 0.5f);

    auto qt_s = [](float v) -> int32_t {
        return static_cast<int32_t>(std::clamp(std::round(v * 10.0f), -200000.0f, 200000.0f));
    };
    auto qt_u = [](float v) -> uint32_t {
        return static_cast<uint32_t>(std::clamp(std::round(v * 10.0f), 0.0f, 200000.0f));
    };
    SheenKey key{qt_s(x), qt_s(y), qt_u(w), qt_u(h), qt_u(rad), qt_u(sh)};
    ID2D1PathGeometry* raw_geo = nullptr;
    auto it = ctx->sheen_cache.find(key);
    if (it != ctx->sheen_cache.end()) {
        raw_geo = it->second.Get();
    } else {
        if (ctx->sheen_cache.size() >= kMaxSheenCacheEntries) {
            ctx->sheen_cache.clear();
        }
        float qx = static_cast<float>(key.x_t) * 0.1f;
        float qy = static_cast<float>(key.y_t) * 0.1f;
        float qw = static_cast<float>(key.w_t) * 0.1f;
        float qrad = static_cast<float>(key.r_t) * 0.1f;
        float qsh = static_cast<float>(key.sh_t) * 0.1f;

        ComPtr<ID2D1PathGeometry> geo;
        if (FAILED(ctx->d2d_factory->CreatePathGeometry(&geo))) return;
        ComPtr<ID2D1GeometrySink> sink;
        if (FAILED(geo->Open(&sink))) return;
        sink->SetFillMode(D2D1_FILL_MODE_WINDING);

        if (qrad <= 0.05f) {
            sink->BeginFigure(D2D1::Point2F(qx, qy), D2D1_FIGURE_BEGIN_FILLED);
            sink->AddLine(D2D1::Point2F(qx + qw, qy));
            sink->AddLine(D2D1::Point2F(qx + qw, qy + qsh));
            sink->AddLine(D2D1::Point2F(qx, qy + qsh));
            sink->EndFigure(D2D1_FIGURE_END_CLOSED);
        } else if (qsh >= qrad) {
            sink->BeginFigure(D2D1::Point2F(qx, qy + qsh), D2D1_FIGURE_BEGIN_FILLED);
            sink->AddLine(D2D1::Point2F(qx, qy + qrad));
            sink->AddArc(D2D1::ArcSegment(
                D2D1::Point2F(qx + qrad, qy), D2D1::SizeF(qrad, qrad), 0.0f,
                D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
            if (qw - 2.0f * qrad > 0.05f) {
                sink->AddLine(D2D1::Point2F(qx + qw - qrad, qy));
            }
            sink->AddArc(D2D1::ArcSegment(
                D2D1::Point2F(qx + qw, qy + qrad), D2D1::SizeF(qrad, qrad), 0.0f,
                D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
            sink->AddLine(D2D1::Point2F(qx + qw, qy + qsh));
            sink->EndFigure(D2D1_FIGURE_END_CLOSED);
        } else {
            float dy = qrad - qsh;
            float hx = std::sqrt(std::max(0.0f, qrad * qrad - dy * dy));
            float lx = qx + qrad - hx;
            float rx = qx + qw - qrad + hx;
            sink->BeginFigure(D2D1::Point2F(lx, qy + qsh), D2D1_FIGURE_BEGIN_FILLED);
            sink->AddArc(D2D1::ArcSegment(
                D2D1::Point2F(qx + qrad, qy), D2D1::SizeF(qrad, qrad), 0.0f,
                D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
            if (qw - 2.0f * qrad > 0.05f) {
                sink->AddLine(D2D1::Point2F(qx + qw - qrad, qy));
            }
            sink->AddArc(D2D1::ArcSegment(
                D2D1::Point2F(rx, qy + qsh), D2D1::SizeF(qrad, qrad), 0.0f,
                D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
            sink->EndFigure(D2D1_FIGURE_END_CLOSED);
        }
        sink->Close();
        raw_geo = geo.Get();
        ctx->sheen_cache.emplace(key, std::move(geo));
    }

    D2dGradStop stops[2] = {
        {0.0f, top_col},
        {1.0f, D2dColor{top_col.r, top_col.g, top_col.b, 0.0f}},
    };
    auto brush = make_linear_brush(ctx, x, y, x, y + sh, stops, 2);
    if (raw_geo && brush) {
        ctx->d2d_ctx->FillGeometry(raw_geo, brush.Get());
    }
}

void d2d_draw_color_sv(
    D2dCtx* ctx, float x, float y, float w, float h, float r, D2dColor hue_col, float alpha) {
    if (!ctx || !ctx->d2d_ctx || w <= 1.0f || h <= 1.0f || alpha <= 0.002f) return;
    ID2D1PathGeometry* geo = get_rounded_rect_hole_geometry(ctx, x, y, w, h, r, 0.0f);
    if (!geo) return;

    D2D1_LAYER_PARAMETERS1 lp = D2D1::LayerParameters1();
    lp.contentBounds = D2D1::RectF(x - 1.0f, y - 1.0f, x + w + 1.0f, y + h + 1.0f);
    lp.geometricMask = geo;
    lp.maskAntialiasMode = D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
    lp.maskTransform = D2D1::IdentityMatrix();
    lp.opacity = std::clamp(alpha, 0.0f, 1.0f);
    ctx->d2d_ctx->PushLayer(&lp, nullptr);

    D2D1_RECT_F rc = D2D1::RectF(x, y, x + w, y + h);
    ctx->solid_brush->SetColor(D2D1::ColorF(hue_col.r, hue_col.g, hue_col.b, 1.0f));
    ctx->d2d_ctx->FillRectangle(rc, ctx->solid_brush.Get());

    D2dGradStop sat_stops[2] = {
        {0.0f, D2dColor{1.0f, 1.0f, 1.0f, 1.0f}},
        {1.0f, D2dColor{1.0f, 1.0f, 1.0f, 0.0f}},
    };
    if (auto sat_brush = make_linear_brush(ctx, x, y, x + w, y, sat_stops, 2)) {
        ctx->d2d_ctx->FillRectangle(rc, sat_brush.Get());
    }

    D2dGradStop val_stops[2] = {
        {0.0f, D2dColor{0.0f, 0.0f, 0.0f, 0.0f}},
        {1.0f, D2dColor{0.0f, 0.0f, 0.0f, 1.0f}},
    };
    if (auto val_brush = make_linear_brush(ctx, x, y, x, y + h, val_stops, 2)) {
        ctx->d2d_ctx->FillRectangle(rc, val_brush.Get());
    }

    ctx->d2d_ctx->PopLayer();
}

void d2d_draw_color_hue(D2dCtx* ctx, float x, float y, float w, float h, float alpha) {
    if (!ctx || !ctx->d2d_ctx || w <= 1.0f || h <= 1.0f || alpha <= 0.002f) return;
    float a = std::clamp(alpha, 0.0f, 1.0f);
    D2dGradStop stops[7] = {
        {0.0f,          D2dColor{1.0f, 0.0f, 0.0f, a}},
        {1.0f / 6.0f,   D2dColor{1.0f, 1.0f, 0.0f, a}},
        {2.0f / 6.0f,   D2dColor{0.0f, 1.0f, 0.0f, a}},
        {3.0f / 6.0f,   D2dColor{0.0f, 1.0f, 1.0f, a}},
        {4.0f / 6.0f,   D2dColor{0.0f, 0.0f, 1.0f, a}},
        {5.0f / 6.0f,   D2dColor{1.0f, 0.0f, 1.0f, a}},
        {1.0f,          D2dColor{1.0f, 0.0f, 0.0f, a}},
    };
    auto brush = make_linear_brush(ctx, x, y + h * 0.5f, x + w, y + h * 0.5f, stops, 7);
    if (!brush) return;
    float rad = h * 0.5f;
    ctx->d2d_ctx->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x, y, x + w, y + h), rad, rad), brush.Get());
}

void d2d_fill_annulus_backdrop(D2dCtx* ctx, float inner_r, float outer_r, float alpha) {
    if (!ctx || !prepare_backdrop_brush(ctx, alpha)) return;
    ID2D1PathGeometry* geo = get_annulus_geometry(ctx, inner_r, outer_r);
    if (geo) ctx->d2d_ctx->FillGeometry(geo, ctx->backdrop_brush.Get());
}

void d2d_fill_annulus_shadow(D2dCtx* ctx, float cy_off, float outer_r, float spread, D2dColor shadow_col) {
    if (!ctx || !ctx->d2d_ctx || shadow_col.a <= 0.002f || spread <= 0.1f) return;
    float r_max = outer_r + spread;
    D2D1_GRADIENT_STOP stops[2] = {
        D2D1::GradientStop(outer_r / r_max, to_d2d(shadow_col)),
        D2D1::GradientStop(1.0f, D2D1::ColorF(shadow_col.r, shadow_col.g, shadow_col.b, 0.0f)),
    };
    ComPtr<ID2D1GradientStopCollection> coll;
    if (FAILED(ctx->d2d_ctx->CreateGradientStopCollection(
            stops, 2, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP, &coll))) return;
    ComPtr<ID2D1RadialGradientBrush> rbrush;
    if (FAILED(ctx->d2d_ctx->CreateRadialGradientBrush(
            D2D1::RadialGradientBrushProperties(
                D2D1::Point2F(0.0f, cy_off), D2D1::Point2F(0.0f, 0.0f), r_max, r_max),
            coll.Get(), &rbrush))) return;

    d2d_save(ctx);
    d2d_translate(ctx, 0.0f, cy_off);
    // Re-center radial brush in shifted local space
    rbrush->SetCenter(D2D1::Point2F(0.0f, 0.0f));
    ID2D1PathGeometry* geo = get_annulus_geometry(ctx, outer_r, r_max);
    if (geo) ctx->d2d_ctx->FillGeometry(geo, rbrush.Get());
    d2d_restore(ctx);
}

void d2d_fill_annulus_linear(
    D2dCtx* ctx, float inner_r, float outer_r,
    float x0, float y0, float x1, float y1,
    const D2dGradStop* stops, int count) {
    if (!ctx || !ctx->d2d_ctx) return;
    ID2D1PathGeometry* geo = get_annulus_geometry(ctx, inner_r, outer_r);
    auto brush = make_linear_brush(ctx, x0, y0, x1, y1, stops, count);
    if (geo && brush) ctx->d2d_ctx->FillGeometry(geo, brush.Get());
}

void d2d_stroke_annulus_rim(
    D2dCtx* ctx, float inner_r, float outer_r, float line_w,
    float x0, float y0, float x1, float y1, D2dColor top_col, D2dColor bot_col) {
    if (!ctx || !ctx->d2d_ctx) return;
    D2dGradStop stops[2] = {{0.0f, top_col}, {1.0f, bot_col}};
    auto brush = make_linear_brush(ctx, x0, y0, x1, y1, stops, 2);
    if (!brush) return;
    if (outer_r > 0.1f) {
        ctx->d2d_ctx->DrawEllipse(D2D1::Ellipse(D2D1::Point2F(0, 0), outer_r, outer_r), brush.Get(), line_w);
    }
    if (inner_r > 0.1f) {
        ctx->d2d_ctx->DrawEllipse(D2D1::Ellipse(D2D1::Point2F(0, 0), inner_r, inner_r), brush.Get(), line_w);
    }
}

void d2d_fill_sector(D2dCtx* ctx, float inner_r, float outer_r, float start_rad, float end_rad, D2dColor fill_col) {
    if (!ctx || !ctx->d2d_ctx || fill_col.a <= 0.002f) return;
    auto geo = make_sector_geometry(ctx->d2d_factory.Get(), inner_r, outer_r, start_rad, end_rad);
    if (!geo) return;
    ctx->solid_brush->SetColor(to_d2d(fill_col));
    ctx->d2d_ctx->FillGeometry(geo.Get(), ctx->solid_brush.Get());
}

void d2d_stroke_arc(
    D2dCtx* ctx, float cx, float cy, float r, float start_rad, float end_rad,
    float line_w, D2dColor col, int round_cap) {
    if (!ctx || !ctx->d2d_ctx || col.a <= 0.002f || r <= 0.1f) return;
    float sweep = end_rad - start_rad;
    if (sweep <= 1e-5f) return;
    ctx->solid_brush->SetColor(to_d2d(col));
    ID2D1StrokeStyle* style = round_cap ? ctx->round_stroke.Get() : ctx->flat_stroke.Get();
    if (sweep >= kTau - 0.0005f) {
        ctx->d2d_ctx->DrawEllipse(D2D1::Ellipse(D2D1::Point2F(cx, cy), r, r), ctx->solid_brush.Get(), line_w, style);
        return;
    }
    auto qt_s = [](float v) -> int32_t {
        return static_cast<int32_t>(std::clamp(std::round(v * 10.0f), -200000.0f, 200000.0f));
    };
    auto qt_u = [](float v) -> uint32_t {
        return static_cast<uint32_t>(std::clamp(std::round(v * 10.0f), 0.0f, 200000.0f));
    };
    auto qt_rad = [](float v) -> int32_t {
        return static_cast<int32_t>(std::clamp(std::round(v * 1000.0f), -200000.0f, 200000.0f));
    };
    ArcKey key{qt_s(cx), qt_s(cy), qt_u(r), qt_rad(start_rad), qt_rad(end_rad)};
    ID2D1PathGeometry* raw_geo = nullptr;
    auto it = ctx->arc_cache.find(key);
    if (it != ctx->arc_cache.end()) {
        raw_geo = it->second.Get();
    } else {
        if (ctx->arc_cache.size() >= kMaxArcCacheEntries) {
            ctx->arc_cache.clear();
        }
        float qcx = static_cast<float>(key.cx_t) * 0.1f;
        float qcy = static_cast<float>(key.cy_t) * 0.1f;
        float qr = static_cast<float>(key.r_t) * 0.1f;
        float qs = static_cast<float>(key.s_mrad) * 0.001f;
        float qe = static_cast<float>(key.e_mrad) * 0.001f;
        float qsweep = qe - qs;
        ComPtr<ID2D1PathGeometry> geo;
        if (FAILED(ctx->d2d_factory->CreatePathGeometry(&geo))) return;
        ComPtr<ID2D1GeometrySink> sink;
        if (FAILED(geo->Open(&sink))) return;
        float x0 = qcx + qr * std::cos(qs);
        float y0 = qcy + qr * std::sin(qs);
        float x1 = qcx + qr * std::cos(qe);
        float y1 = qcy + qr * std::sin(qe);
        sink->BeginFigure(D2D1::Point2F(x0, y0), D2D1_FIGURE_BEGIN_HOLLOW);
        sink->AddArc(D2D1::ArcSegment(
            D2D1::Point2F(x1, y1), D2D1::SizeF(qr, qr), 0.0f,
            D2D1_SWEEP_DIRECTION_CLOCKWISE,
            (qsweep > kPi) ? D2D1_ARC_SIZE_LARGE : D2D1_ARC_SIZE_SMALL));
        sink->EndFigure(D2D1_FIGURE_END_OPEN);
        sink->Close();
        raw_geo = geo.Get();
        ctx->arc_cache.emplace(key, std::move(geo));
    }
    if (raw_geo) {
        ctx->d2d_ctx->DrawGeometry(raw_geo, ctx->solid_brush.Get(), line_w, style);
    }
}

void d2d_draw_line(D2dCtx* ctx, float x0, float y0, float x1, float y1, float line_w, D2dColor col, int round_cap) {
    if (!ctx || !ctx->d2d_ctx || col.a <= 0.002f) return;
    ctx->solid_brush->SetColor(to_d2d(col));
    ID2D1StrokeStyle* style = round_cap ? ctx->round_stroke.Get() : ctx->flat_stroke.Get();
    ctx->d2d_ctx->DrawLine(D2D1::Point2F(x0, y0), D2D1::Point2F(x1, y1), ctx->solid_brush.Get(), line_w, style);
}

void d2d_draw_line_gradient(
    D2dCtx* ctx, float x0, float y0, float x1, float y1, float line_w,
    const D2dGradStop* stops, int count) {
    if (!ctx || !ctx->d2d_ctx) return;
    auto brush = make_linear_brush(ctx, x0, y0, x1, y1, stops, count);
    if (!brush) return;
    ctx->d2d_ctx->DrawLine(D2D1::Point2F(x0, y0), D2D1::Point2F(x1, y1), brush.Get(), line_w);
}

void d2d_fill_rounded_rect_backdrop(
    D2dCtx* ctx, float x, float y, float w, float h, float r, float hole_r, float alpha) {
    if (!ctx || !prepare_backdrop_brush(ctx, alpha)) return;
    if (hole_r <= 0.05f) {
        float rad = std::clamp(r, 0.0f, std::min(w, h) * 0.5f);
        ctx->d2d_ctx->FillRoundedRectangle(
            D2D1::RoundedRect(D2D1::RectF(x, y, x + w, y + h), rad, rad),
            ctx->backdrop_brush.Get());
        return;
    }
    ID2D1PathGeometry* geo = get_rounded_rect_hole_geometry(ctx, x, y, w, h, r, hole_r);
    if (geo) ctx->d2d_ctx->FillGeometry(geo, ctx->backdrop_brush.Get());
}

void d2d_fill_rounded_rect_solid(D2dCtx* ctx, float x, float y, float w, float h, float r, D2dColor col) {
    if (!ctx || !ctx->d2d_ctx || col.a <= 0.002f) return;
    ctx->solid_brush->SetColor(to_d2d(col));
    float rad = std::clamp(r, 0.0f, std::min(w, h) * 0.5f);
    ctx->d2d_ctx->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x, y, x + w, y + h), rad, rad), ctx->solid_brush.Get());
}

void d2d_fill_rounded_rect_linear(
    D2dCtx* ctx, float x, float y, float w, float h, float r, float hole_r,
    float gx0, float gy0, float gx1, float gy1, const D2dGradStop* stops, int count) {
    if (!ctx || !ctx->d2d_ctx) return;
    auto brush = make_linear_brush(ctx, gx0, gy0, gx1, gy1, stops, count);
    if (!brush) return;
    if (hole_r > 0.05f) {
        ID2D1PathGeometry* geo = get_rounded_rect_hole_geometry(ctx, x, y, w, h, r, hole_r);
        if (geo) ctx->d2d_ctx->FillGeometry(geo, brush.Get());
    } else {
        float rad = std::clamp(r, 0.0f, std::min(w, h) * 0.5f);
        ctx->d2d_ctx->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x, y, x + w, y + h), rad, rad), brush.Get());
    }
}

void d2d_stroke_rounded_rect(D2dCtx* ctx, float x, float y, float w, float h, float r, float line_w, D2dColor col) {
    if (!ctx || !ctx->d2d_ctx || col.a <= 0.002f) return;
    ctx->solid_brush->SetColor(to_d2d(col));
    float rad = std::clamp(r, 0.0f, std::min(w, h) * 0.5f);
    ctx->d2d_ctx->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x, y, x + w, y + h), rad, rad), ctx->solid_brush.Get(), line_w);
}

void d2d_stroke_rounded_rect_linear(
    D2dCtx* ctx, float x, float y, float w, float h, float r, float line_w,
    float gx0, float gy0, float gx1, float gy1, D2dColor c0, D2dColor c1) {
    if (!ctx || !ctx->d2d_ctx) return;
    D2dGradStop stops[2] = {{0.0f, c0}, {1.0f, c1}};
    auto brush = make_linear_brush(ctx, gx0, gy0, gx1, gy1, stops, 2);
    if (!brush) return;
    float rad = std::clamp(r, 0.0f, std::min(w, h) * 0.5f);
    ctx->d2d_ctx->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x, y, x + w, y + h), rad, rad), brush.Get(), line_w);
}

void d2d_fill_rounded_rect_shadow(
    D2dCtx* ctx, float x, float y, float w, float h, float r, D2dColor shadow_col, float alpha_scale) {
    if (!ctx || !ctx->d2d_ctx || alpha_scale <= 0.002f) return;
    // Soft graduated outer shadow strictly outside the rounded glass outline (matches Linux capsule & box shadow)
    const float alphas[3] = {0.065f, 0.038f, 0.018f};
    const auto* geos = get_rounded_rect_shadow_geometries(ctx, x, y, w, h, r);
    if (!geos) return;
    for (size_t i = 0; i < 3; ++i) {
        if (!(*geos)[i]) continue;
        ctx->solid_brush->SetColor(D2D1::ColorF(shadow_col.r, shadow_col.g, shadow_col.b, alphas[i] * alpha_scale));
        ctx->d2d_ctx->FillGeometry((*geos)[i].Get(), ctx->solid_brush.Get());
    }
}

void d2d_fill_circle(D2dCtx* ctx, float cx, float cy, float r, D2dColor col) {
    if (!ctx || !ctx->d2d_ctx || col.a <= 0.002f || r <= 0.05f) return;
    ctx->solid_brush->SetColor(to_d2d(col));
    ctx->d2d_ctx->FillEllipse(D2D1::Ellipse(D2D1::Point2F(cx, cy), r, r), ctx->solid_brush.Get());
}

void d2d_stroke_circle(D2dCtx* ctx, float cx, float cy, float r, float line_w, D2dColor col) {
    if (!ctx || !ctx->d2d_ctx || col.a <= 0.002f || r <= 0.05f) return;
    ctx->solid_brush->SetColor(to_d2d(col));
    ctx->d2d_ctx->DrawEllipse(D2D1::Ellipse(D2D1::Point2F(cx, cy), r, r), ctx->solid_brush.Get(), line_w);
}

void d2d_stroke_circle_linear(
    D2dCtx* ctx, float cx, float cy, float r, float line_w,
    float gx0, float gy0, float gx1, float gy1, D2dColor c0, D2dColor c1) {
    if (!ctx || !ctx->d2d_ctx || r <= 0.05f) return;
    D2dGradStop stops[2] = {{0.0f, c0}, {1.0f, c1}};
    auto brush = make_linear_brush(ctx, gx0, gy0, gx1, gy1, stops, 2);
    if (!brush) return;
    ctx->d2d_ctx->DrawEllipse(D2D1::Ellipse(D2D1::Point2F(cx, cy), r, r), brush.Get(), line_w);
}

void d2d_fill_polygon(D2dCtx* ctx, const float* xy, int count, D2dColor col) {
    if (!ctx || !ctx->d2d_ctx || !xy || count < 3 || col.a <= 0.002f) return;
    ComPtr<ID2D1PathGeometry> geo;
    if (FAILED(ctx->d2d_factory->CreatePathGeometry(&geo))) return;
    ComPtr<ID2D1GeometrySink> sink;
    if (FAILED(geo->Open(&sink))) return;
    sink->BeginFigure(D2D1::Point2F(xy[0], xy[1]), D2D1_FIGURE_BEGIN_FILLED);
    for (int i = 1; i < count; ++i) {
        sink->AddLine(D2D1::Point2F(xy[i * 2], xy[i * 2 + 1]));
    }
    sink->EndFigure(D2D1_FIGURE_END_CLOSED);
    sink->Close();
    ctx->solid_brush->SetColor(to_d2d(col));
    ctx->d2d_ctx->FillGeometry(geo.Get(), ctx->solid_brush.Get());
}

void d2d_stroke_polygon(D2dCtx* ctx, const float* xy, int count, float line_w, D2dColor col) {
    if (!ctx || !ctx->d2d_ctx || !xy || count < 3 || col.a <= 0.002f) return;
    ComPtr<ID2D1PathGeometry> geo;
    if (FAILED(ctx->d2d_factory->CreatePathGeometry(&geo))) return;
    ComPtr<ID2D1GeometrySink> sink;
    if (FAILED(geo->Open(&sink))) return;
    sink->BeginFigure(D2D1::Point2F(xy[0], xy[1]), D2D1_FIGURE_BEGIN_HOLLOW);
    for (int i = 1; i < count; ++i) {
        sink->AddLine(D2D1::Point2F(xy[i * 2], xy[i * 2 + 1]));
    }
    sink->EndFigure(D2D1_FIGURE_END_CLOSED);
    sink->Close();
    ctx->solid_brush->SetColor(to_d2d(col));
    ctx->d2d_ctx->DrawGeometry(geo.Get(), ctx->solid_brush.Get(), line_w, ctx->round_stroke.Get());
}

void d2d_fill_polygon_ring_backdrop(
    D2dCtx* ctx, const float* outer_xy, int outer_n, const float* inner_xy, int inner_n, float alpha) {
    if (!ctx || !prepare_backdrop_brush(ctx, alpha)) return;
    auto geo = make_polygon_ring_geometry(ctx->d2d_factory.Get(), outer_xy, outer_n, inner_xy, inner_n);
    if (geo) ctx->d2d_ctx->FillGeometry(geo.Get(), ctx->backdrop_brush.Get());
}

void d2d_fill_polygon_ring_linear(
    D2dCtx* ctx, const float* outer_xy, int outer_n, const float* inner_xy, int inner_n,
    float gx0, float gy0, float gx1, float gy1, const D2dGradStop* stops, int count) {
    if (!ctx || !ctx->d2d_ctx) return;
    auto geo = make_polygon_ring_geometry(ctx->d2d_factory.Get(), outer_xy, outer_n, inner_xy, inner_n);
    auto brush = make_linear_brush(ctx, gx0, gy0, gx1, gy1, stops, count);
    if (geo && brush) ctx->d2d_ctx->FillGeometry(geo.Get(), brush.Get());
}

void d2d_stroke_polygon_ring_linear(
    D2dCtx* ctx, const float* outer_xy, int outer_n, const float* inner_xy, int inner_n,
    float line_w, float gx0, float gy0, float gx1, float gy1, D2dColor c0, D2dColor c1) {
    if (!ctx || !ctx->d2d_ctx) return;
    auto geo = make_polygon_ring_geometry(ctx->d2d_factory.Get(), outer_xy, outer_n, inner_xy, inner_n);
    D2dGradStop stops[2] = {{0.0f, c0}, {1.0f, c1}};
    auto brush = make_linear_brush(ctx, gx0, gy0, gx1, gy1, stops, 2);
    if (geo && brush) ctx->d2d_ctx->DrawGeometry(geo.Get(), brush.Get(), line_w, ctx->round_stroke.Get());
}

void d2d_draw_path(
    D2dCtx* ctx, const D2dPathCmd* cmds, int count,
    int do_fill, D2dColor fill_col,
    int do_stroke, D2dColor stroke_col, float stroke_w, int round_cap) {
    if (!ctx || !ctx->d2d_ctx || !cmds || count <= 0) return;

    auto build_path_geo = [&]() -> ComPtr<ID2D1PathGeometry> {
        ComPtr<ID2D1PathGeometry> geo;
        if (FAILED(ctx->d2d_factory->CreatePathGeometry(&geo))) return nullptr;
        ComPtr<ID2D1GeometrySink> sink;
        if (FAILED(geo->Open(&sink))) return nullptr;
        sink->SetFillMode(D2D1_FILL_MODE_WINDING);

        bool in_figure = false;
        D2D1_FIGURE_BEGIN fig_begin = do_fill ? D2D1_FIGURE_BEGIN_FILLED : D2D1_FIGURE_BEGIN_HOLLOW;

        auto close_open = [&]() {
            if (in_figure) {
                sink->EndFigure(D2D1_FIGURE_END_OPEN);
                in_figure = false;
            }
        };

        for (int i = 0; i < count; ++i) {
            const auto& c = cmds[i];
            switch (c.op) {
                case 0: // MoveTo
                    close_open();
                    sink->BeginFigure(D2D1::Point2F(c.x1, c.y1), fig_begin);
                    in_figure = true;
                    break;
                case 1: // LineTo
                    if (!in_figure) {
                        sink->BeginFigure(D2D1::Point2F(c.x1, c.y1), fig_begin);
                        in_figure = true;
                    } else {
                        sink->AddLine(D2D1::Point2F(c.x1, c.y1));
                    }
                    break;
                case 2: // CubicTo
                    if (in_figure) {
                        sink->AddBezier(D2D1::BezierSegment(
                            D2D1::Point2F(c.x1, c.y1),
                            D2D1::Point2F(c.x2, c.y2),
                            D2D1::Point2F(c.x3, c.y3)));
                    }
                    break;
                case 3: { // Standalone Arc(cx=x1, cy=y1, r=x2, start_rad=y2, end_rad=x3)
                    close_open();
                    float cx = c.x1, cy = c.y1, r = c.x2, s = c.y2, e = c.x3;
                    float sweep = e - s;
                    if (r > 0.01f && sweep > 1e-4f) {
                        if (sweep >= kTau - 0.001f) {
                            sink->BeginFigure(D2D1::Point2F(cx + r, cy), fig_begin);
                            sink->AddArc(D2D1::ArcSegment(
                                D2D1::Point2F(cx - r, cy), D2D1::SizeF(r, r), 0.0f,
                                D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
                            sink->AddArc(D2D1::ArcSegment(
                                D2D1::Point2F(cx + r, cy), D2D1::SizeF(r, r), 0.0f,
                                D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
                            sink->EndFigure(D2D1_FIGURE_END_CLOSED);
                        } else {
                            sink->BeginFigure(D2D1::Point2F(cx + r * std::cos(s), cy + r * std::sin(s)), fig_begin);
                            sink->AddArc(D2D1::ArcSegment(
                                D2D1::Point2F(cx + r * std::cos(e), cy + r * std::sin(e)),
                                D2D1::SizeF(r, r), 0.0f,
                                D2D1_SWEEP_DIRECTION_CLOCKWISE,
                                (sweep > kPi) ? D2D1_ARC_SIZE_LARGE : D2D1_ARC_SIZE_SMALL));
                            in_figure = true;
                        }
                    }
                    break;
                }
                case 4: // CloseFigure
                    if (in_figure) {
                        sink->EndFigure(D2D1_FIGURE_END_CLOSED);
                        in_figure = false;
                    }
                    break;
                case 5: // RoundedRect(x=x1, y=y1, w=x2, h=y2, r=x3)
                    close_open();
                    add_rounded_rect_figure(sink.Get(), c.x1, c.y1, c.x2, c.y2, c.x3);
                    break;
                default:
                    break;
            }
        }
        close_open();
        sink->Close();
        return geo;
    };

    ComPtr<ID2D1PathGeometry> uncached_geo;
    ID2D1PathGeometry* raw_geo = nullptr;
    if (count <= 64) {
        auto& pkey = ctx->path_probe_key;
        pkey.filled = do_fill ? 1 : 0;
        pkey.cmds.assign(cmds, cmds + count);
        auto it = ctx->path_cache.find(pkey);
        if (it != ctx->path_cache.end()) {
            raw_geo = it->second.Get();
        } else {
            if (ctx->path_cache.size() >= kMaxPathCacheEntries) {
                ctx->path_cache.clear();
            }
            ComPtr<ID2D1PathGeometry> geo = build_path_geo();
            if (!geo) return;
            raw_geo = geo.Get();
            ctx->path_cache.emplace(pkey, std::move(geo));
        }
    } else {
        uncached_geo = build_path_geo();
        raw_geo = uncached_geo.Get();
    }
    if (!raw_geo) return;

    if (do_fill && fill_col.a > 0.002f) {
        ctx->solid_brush->SetColor(to_d2d(fill_col));
        ctx->d2d_ctx->FillGeometry(raw_geo, ctx->solid_brush.Get());
    }
    if (do_stroke && stroke_col.a > 0.002f && stroke_w > 0.01f) {
        ctx->solid_brush->SetColor(to_d2d(stroke_col));
        ID2D1StrokeStyle* style = round_cap ? ctx->round_stroke.Get() : ctx->flat_stroke.Get();
        ctx->d2d_ctx->DrawGeometry(raw_geo, ctx->solid_brush.Get(), stroke_w, style);
    }
}

void d2d_measure_text(
    D2dCtx* ctx, const uint16_t* text, int len, float size, int bold,
    float max_w, int max_lines, float* out_w, float* out_h) {
    if (!ctx || !text || len <= 0) {
        if (out_w) *out_w = 0.0f;
        if (out_h) *out_h = 0.0f;
        return;
    }
    measure_string_span(
        ctx, reinterpret_cast<const wchar_t*>(text), static_cast<size_t>(len),
        size, bold != 0, max_w, max_lines, out_w, out_h);
}

float d2d_hit_test_text_pos(
    D2dCtx* ctx, const uint16_t* text, int len, float size, int bold, int utf16_pos) {
    if (!ctx || !ctx->dwrite || !text || len <= 0 || utf16_pos <= 0) {
        return 0.0f;
    }
    uint32_t size_q = static_cast<uint32_t>(std::clamp(std::round(size * 10.0f), 50.0f, 1600.0f));
    auto& probe = ctx->layout_probe_key;
    probe.text.assign(reinterpret_cast<const wchar_t*>(text), static_cast<size_t>(len));
    probe.size_tenths = size_q;
    probe.w_half_px = 8192 * 2;
    probe.h_half_px = 256 * 2;
    probe.bold = static_cast<uint8_t>(bold ? 1 : 0);
    probe.align = 0;
    probe.ellipsize = 0;
    probe.wrap_lines = 1;

    IDWriteTextLayout* raw_layout = nullptr;
    auto it = ctx->layout_map.find(probe);
    if (it != ctx->layout_map.end()) {
        ctx->layout_lru.splice(ctx->layout_lru.begin(), ctx->layout_lru, it->second);
        raw_layout = it->second->second.Get();
    } else {
        IDWriteTextFormat* fmt = get_text_format(ctx, size, bold != 0, 0, false);
        if (!fmt) return 0.0f;
        ComPtr<IDWriteTextLayout> layout;
        if (!create_tabular_text_layout(
                ctx, probe.text.c_str(), static_cast<UINT32>(len),
                fmt, 8192.0f, 256.0f, layout)) {
            return 0.0f;
        }
        raw_layout = layout.Get();
        if (ctx->layout_lru.size() >= kMaxLayoutCacheEntries) {
            ctx->layout_map.erase(ctx->layout_lru.back().first);
            ctx->layout_lru.pop_back();
        }
        TextLayoutKey owned_key = probe;
        ctx->layout_lru.emplace_front(std::move(owned_key), std::move(layout));
        ctx->layout_map.emplace(ctx->layout_lru.front().first, ctx->layout_lru.begin());
    }
    FLOAT px = 0.0f, py = 0.0f;
    DWRITE_HIT_TEST_METRICS htm{};
    HRESULT hr = S_OK;
    if (utf16_pos >= len) {
        hr = raw_layout->HitTestTextPosition(static_cast<UINT32>(len - 1), TRUE, &px, &py, &htm);
    } else {
        hr = raw_layout->HitTestTextPosition(static_cast<UINT32>(utf16_pos), FALSE, &px, &py, &htm);
    }
    if (FAILED(hr) || !std::isfinite(px)) {
        return 0.0f;
    }
    return std::max(0.0f, px);
}

void d2d_draw_text(
    D2dCtx* ctx, const uint16_t* text, int len, float size, int bold,
    float x, float y, float w, float h, int align, int ellipsize, int wrap_lines,
    D2dColor col, int has_shadow, D2dColor shadow_col) {
    if (!ctx || !ctx->d2d_ctx || !ctx->dwrite || !text || len <= 0 || col.a <= 0.002f) return;

    float lw = std::max(1.0f, w);
    float lh = std::max(1.0f, h);
    uint32_t size_q = static_cast<uint32_t>(std::clamp(std::round(size * 10.0f), 50.0f, 1600.0f));
    uint32_t w_half = static_cast<uint32_t>(std::clamp(std::round(lw * 2.0f), 2.0f, 32768.0f));
    uint32_t h_half = static_cast<uint32_t>(std::clamp(std::round(lh * 2.0f), 2.0f, 32768.0f));
    uint8_t wrap_clamped = static_cast<uint8_t>(std::clamp(wrap_lines, 1, 16));

    auto& probe = ctx->layout_probe_key;
    probe.text.assign(reinterpret_cast<const wchar_t*>(text), static_cast<size_t>(len));
    probe.size_tenths = size_q;
    probe.w_half_px = w_half;
    probe.h_half_px = h_half;
    probe.bold = static_cast<uint8_t>(bold ? 1 : 0);
    probe.align = static_cast<uint8_t>(align);
    probe.ellipsize = static_cast<uint8_t>(ellipsize);
    probe.wrap_lines = wrap_clamped;

    IDWriteTextLayout* raw_layout = nullptr;
    auto it = ctx->layout_map.find(probe);
    if (it != ctx->layout_map.end()) {
        ctx->layout_lru.splice(ctx->layout_lru.begin(), ctx->layout_lru, it->second);
        raw_layout = it->second->second.Get();
    } else {
        float lw_q = static_cast<float>(w_half) * 0.5f;
        float lh_q = static_cast<float>(h_half) * 0.5f;
        bool wrap = (wrap_clamped > 1);

        std::wstring s = probe.text;
        if (!wrap && (ellipsize == 1 || ellipsize == 2)) {
            s = ellipsize_start_or_middle(ctx, s, size, bold != 0, lw_q, ellipsize);
        }

        // Fetch format entry AFTER ellipsize_start_or_middle so format_cache cannot rehash/clear fmt_entry.
        const CachedFormat* fmt_entry = get_text_format_entry(ctx, size, bold != 0, align, wrap);
        if (!fmt_entry || !fmt_entry->format) return;
        IDWriteTextFormat* fmt = fmt_entry->format.Get();
        IDWriteInlineObject* ellipsis_sign = fmt_entry->ellipsis.Get();

        ComPtr<IDWriteTextLayout> layout;
        if (!create_tabular_text_layout(
                ctx, s.c_str(), static_cast<UINT32>(s.size()), fmt, lw_q, lh_q, layout)) return;

        // DirectWrite's SetTrimming only trims single-line horizontal overflow; for multi-line wrapped
        // blocks (e.g. 2-line app grid labels), enforce <= wrap_lines with an explicit trailing ellipsis.
        if (wrap && ellipsize != 0) {
            UINT32 actual = 0;
            layout->GetLineMetrics(nullptr, 0, &actual);
            if (actual > static_cast<UINT32>(wrap_clamped)) {
                size_t hi = s.size();
                if (actual <= 64) {
                    DWRITE_LINE_METRICS stack_lines[64];
                    UINT32 got_lines = 0;
                    if (SUCCEEDED(layout->GetLineMetrics(stack_lines, actual, &got_lines)) &&
                        got_lines >= static_cast<UINT32>(wrap_clamped)) {
                        size_t max_chars = 0;
                        for (UINT32 li = 0; li < static_cast<UINT32>(wrap_clamped); ++li) {
                            max_chars += stack_lines[li].length;
                        }
                        if (max_chars > 0 && max_chars < hi) {
                            hi = max_chars;
                        }
                    }
                }
                const std::wstring ell = L"\u2026";
                size_t lo = 1;
                ComPtr<IDWriteTextLayout> best_layout;
                while (lo <= hi) {
                    size_t mid = (lo + hi) / 2;
                    size_t trim_end = mid;
                    while (trim_end > 1 && iswspace(s[trim_end - 1])) {
                        --trim_end;
                    }
                    if (trim_end > 0 && s[trim_end - 1] >= 0xD800 && s[trim_end - 1] <= 0xDBFF) {
                        --trim_end;
                    }
                    std::wstring cand = s.substr(0, trim_end) + ell;
                    ComPtr<IDWriteTextLayout> cand_layout;
                    if (!create_tabular_text_layout(
                            ctx, cand.c_str(), static_cast<UINT32>(cand.size()), fmt, lw_q, lh_q, cand_layout)) {
                        break;
                    }
                    UINT32 cand_lines = 0;
                    cand_layout->GetLineMetrics(nullptr, 0, &cand_lines);
                    if (cand_lines <= static_cast<UINT32>(wrap_clamped)) {
                        best_layout = std::move(cand_layout);
                        lo = mid + 1;
                    } else {
                        if (mid == 0) break;
                        hi = mid - 1;
                    }
                }
                if (best_layout) {
                    layout = std::move(best_layout);
                } else {
                    ComPtr<IDWriteTextLayout> ell_layout;
                    if (create_tabular_text_layout(
                            ctx, ell.c_str(), static_cast<UINT32>(ell.size()), fmt, lw_q, lh_q, ell_layout)) {
                        layout = std::move(ell_layout);
                    }
                }
            }
        }

        if (!wrap && ellipsize != 0 && ellipsis_sign) {
            DWRITE_TRIMMING trim{};
            trim.granularity = DWRITE_TRIMMING_GRANULARITY_CHARACTER;
            layout->SetTrimming(&trim, ellipsis_sign);
        }

        raw_layout = layout.Get();
        if (ctx->layout_lru.size() >= kMaxLayoutCacheEntries) {
            ctx->layout_map.erase(ctx->layout_lru.back().first);
            ctx->layout_lru.pop_back();
        }
        TextLayoutKey owned_key = probe;
        ctx->layout_lru.emplace_front(std::move(owned_key), std::move(layout));
        ctx->layout_map.emplace(ctx->layout_lru.front().first, ctx->layout_lru.begin());
    }

    if (!raw_layout) return;
    if (has_shadow && shadow_col.a > 0.002f) {
        ctx->solid_brush->SetColor(to_d2d(shadow_col));
        ctx->d2d_ctx->DrawTextLayout(
            D2D1::Point2F(x, y + 1.0f), raw_layout, ctx->solid_brush.Get(), D2D1_DRAW_TEXT_OPTIONS_CLIP);
    }
    ctx->solid_brush->SetColor(to_d2d(col));
    ctx->d2d_ctx->DrawTextLayout(
        D2D1::Point2F(x, y), raw_layout, ctx->solid_brush.Get(), D2D1_DRAW_TEXT_OPTIONS_CLIP);
}

void d2d_draw_query_text(
    D2dCtx* ctx,
    const uint16_t* text, int text_len,
    const uint16_t* suffix, int suffix_len,
    float size, int bold,
    float x, float y, float w, float h, int align,
    D2dColor main_col, D2dColor ghost_col,
    int has_shadow, D2dColor shadow_col) {
    if (!ctx || !ctx->d2d_ctx || !ctx->dwrite || !text || text_len <= 0 || main_col.a <= 0.002f) return;
    if (!suffix || suffix_len <= 0) {
        d2d_draw_text(ctx, text, text_len, size, bold, x, y, w, h, align, 1, 1, main_col, has_shadow, shadow_col);
        return;
    }
    std::wstring combined(reinterpret_cast<const wchar_t*>(text), static_cast<size_t>(text_len));
    size_t main_u16 = combined.size();
    combined.append(reinterpret_cast<const wchar_t*>(suffix), static_cast<size_t>(suffix_len));

    const CachedFormat* fmt_entry = get_text_format_entry(ctx, size, bold != 0, align, false);
    if (!fmt_entry || !fmt_entry->format) return;
    ComPtr<IDWriteTextLayout> layout;
    if (!create_tabular_text_layout(
            ctx, combined.c_str(), static_cast<UINT32>(combined.size()), fmt_entry->format.Get(),
            std::max(1.0f, w), std::max(1.0f, h), layout)) return;

    if (fmt_entry->ellipsis) {
        DWRITE_TRIMMING trim{};
        trim.granularity = DWRITE_TRIMMING_GRANULARITY_CHARACTER;
        layout->SetTrimming(&trim, fmt_entry->ellipsis.Get());
    }

    if (has_shadow && shadow_col.a > 0.002f) {
        ctx->solid_brush->SetColor(to_d2d(shadow_col));
        ctx->d2d_ctx->DrawTextLayout(
            D2D1::Point2F(x, y + 1.0f), layout.Get(), ctx->solid_brush.Get(), D2D1_DRAW_TEXT_OPTIONS_CLIP);
    }

    if (ctx->ghost_brush) {
        ctx->ghost_brush->SetColor(to_d2d(ghost_col));
        DWRITE_TEXT_RANGE rng{static_cast<UINT32>(main_u16), static_cast<UINT32>(combined.size() - main_u16)};
        layout->SetDrawingEffect(ctx->ghost_brush.Get(), rng);
    }

    ctx->solid_brush->SetColor(to_d2d(main_col));
    ctx->d2d_ctx->DrawTextLayout(
        D2D1::Point2F(x, y), layout.Get(), ctx->solid_brush.Get(), D2D1_DRAW_TEXT_OPTIONS_CLIP);
}

int d2d_create_bitmap_from_hicon(D2dCtx* ctx, HICON hicon) {
    if (!ensure_wic(ctx) || !hicon) return 0;
    ComPtr<IWICBitmap> wic_bmp;
    if (FAILED(ctx->wic->CreateBitmapFromHICON(hicon, &wic_bmp)) || !wic_bmp) return 0;
    return register_wic_bitmap(ctx, wic_bmp.Get(), 128);
}

static std::wstring expand_env_w(const wchar_t* src) {
    if (!src || !*src) return {};
    DWORD need = ExpandEnvironmentStringsW(src, nullptr, 0);
    if (need == 0 || need > 32768) return src;
    std::wstring out(need, L'\0');
    DWORD got = ExpandEnvironmentStringsW(src, out.data(), need);
    if (got == 0) return src;
    while (!out.empty() && out.back() == L'\0') out.pop_back();
    return out;
}

static HICON extract_hires_icon(const std::wstring& file, int index, int desired_px) {
    if (file.empty()) return nullptr;
    HICON hicon = nullptr;
    UINT id = 0;
    UINT got = PrivateExtractIconsW(file.c_str(), index, desired_px, desired_px, &hicon, &id, 1, LR_DEFAULTCOLOR);
    if (got != 0 && got != 0xFFFFFFFF && hicon != nullptr) {
        return hicon;
    }
    return nullptr;
}

static HICON extract_shell_hicon(
    const wchar_t* path,
    int desired_px,
    IShellLinkW* reusable_link,
    IPersistFile* reusable_pf) {
    if (!path || !*path) return nullptr;
    int sz = std::clamp(desired_px, 32, 256);

    auto has_ext_ci = [](const std::wstring& s, const wchar_t* ext) -> bool {
        size_t el = wcslen(ext);
        if (s.size() < el) return false;
        return _wcsicmp(s.c_str() + (s.size() - el), ext) == 0;
    };

    HICON hicon = nullptr;
    std::wstring pstr(path);

    // 1. If .lnk shortcut, resolve via IShellLinkW so we extract clean crisp HiDPI app icons
    //    WITHOUT Windows's giant white/blue shortcut arrow overlay!
    if (has_ext_ci(pstr, L".lnk")) {
        ComPtr<IShellLinkW> local_link;
        ComPtr<IPersistFile> local_pf;
        IShellLinkW* link = reusable_link;
        IPersistFile* pf = reusable_pf;
        if (!link || !pf) {
            if (SUCCEEDED(CoCreateInstance(CLSID_ShellLink, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&local_link))) && local_link) {
                local_link.As(&local_pf);
                link = local_link.Get();
                pf = local_pf.Get();
            }
        }
        if (link && pf) {
            // Reset previous fields so reused IShellLinkW cannot bleed state across shortcuts
            link->SetIconLocation(L"", 0);
            link->SetPath(L"");
            if (SUCCEEDED(pf->Load(path, STGM_READ))) {
                wchar_t icon_loc[MAX_PATH * 2]{};
                int icon_idx = 0;
                if (SUCCEEDED(link->GetIconLocation(icon_loc, MAX_PATH * 2, &icon_idx)) && icon_loc[0] != L'\0') {
                    std::wstring expanded = expand_env_w(icon_loc);
                    hicon = extract_hires_icon(expanded, icon_idx, sz);
                }
                if (!hicon) {
                    wchar_t target[MAX_PATH * 2]{};
                    if (SUCCEEDED(link->GetPath(target, MAX_PATH * 2, nullptr, SLGP_RAWPATH)) && target[0] != L'\0') {
                        std::wstring expanded = expand_env_w(target);
                        hicon = extract_hires_icon(expanded, 0, sz);
                        if (!hicon && !expanded.empty()) {
                            SHFILEINFOW sfi{};
                            if (SHGetFileInfoW(expanded.c_str(), 0, &sfi, sizeof(sfi), SHGFI_ICON | SHGFI_LARGEICON) != 0) {
                                hicon = sfi.hIcon;
                            }
                        }
                    }
                }
            }
        }
    } else {
        // 2. Direct .exe / .dll / .ico: extract crisp HiDPI icon directly
        hicon = extract_hires_icon(pstr, 0, sz);
    }

    // 3. Fallback to SHGetFileInfoW (handles UWP/shell-only items or .url shortcuts cleanly)
    if (!hicon) {
        SHFILEINFOW sfi{};
        if (SHGetFileInfoW(path, 0, &sfi, sizeof(sfi), SHGFI_ICON | SHGFI_LARGEICON) != 0) {
            hicon = sfi.hIcon;
        }
    }
    return hicon;
}

int d2d_create_bitmap_from_shell_path(D2dCtx* ctx, const uint16_t* wide_path, int desired_px) {
    if (!ensure_wic(ctx) || !wide_path || !*wide_path) return 0;
    const wchar_t* path = reinterpret_cast<const wchar_t*>(wide_path);
    HICON hicon = extract_shell_hicon(path, desired_px, nullptr, nullptr);
    if (!hicon) return 0;
    int bmp_id = d2d_create_bitmap_from_hicon(ctx, hicon);
    DestroyIcon(hicon);
    return bmp_id;
}

struct ShellIconWorker {
    bool com_inited = false;
    ComPtr<IWICImagingFactory> wic;
    ComPtr<IShellLinkW> shell_link;
    ComPtr<IPersistFile> persist_file;
};

ShellIconWorker* d2d_icon_worker_create() {
    auto* w = new ShellIconWorker();
    HRESULT hr_com = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (SUCCEEDED(hr_com)) {
        w->com_inited = true;
    }
    CoCreateInstance(
        CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER,
        IID_PPV_ARGS(&w->wic));
    if (SUCCEEDED(CoCreateInstance(
            CLSID_ShellLink, nullptr, CLSCTX_INPROC_SERVER,
            IID_PPV_ARGS(&w->shell_link))) && w->shell_link) {
        w->shell_link.As(&w->persist_file);
    }
    return w;
}

void d2d_icon_worker_destroy(ShellIconWorker* worker) {
    if (!worker) return;
    bool com = worker->com_inited;
    worker->persist_file.Reset();
    worker->shell_link.Reset();
    worker->wic.Reset();
    delete worker;
    if (com) {
        CoUninitialize();
    }
}

int d2d_icon_worker_extract_bgra(
    ShellIconWorker* worker,
    const uint16_t* wide_path,
    int desired_px,
    uint8_t* out_bgra,
    int max_bytes,
    int* out_w,
    int* out_h) {
    if (!worker || !worker->wic || !wide_path || !*wide_path || !out_bgra || max_bytes <= 0) {
        return 0;
    }
    int sz = std::clamp(desired_px, 32, 256);
    const wchar_t* path = reinterpret_cast<const wchar_t*>(wide_path);
    HICON hicon = extract_shell_hicon(path, sz, worker->shell_link.Get(), worker->persist_file.Get());
    if (!hicon) return 0;

    ComPtr<IWICBitmap> wic_bmp;
    HRESULT hr = worker->wic->CreateBitmapFromHICON(hicon, &wic_bmp);
    DestroyIcon(hicon);
    if (FAILED(hr) || !wic_bmp) return 0;

    UINT w = 0, h = 0;
    if (FAILED(wic_bmp->GetSize(&w, &h)) || w == 0 || h == 0) return 0;

    ComPtr<IWICBitmapSource> current = wic_bmp;
    UINT dst_w = w, dst_h = h;
    if (static_cast<int>(w) > sz || static_cast<int>(h) > sz) {
        double scale = static_cast<double>(sz) / static_cast<double>(std::max(w, h));
        dst_w = std::max<UINT>(1, static_cast<UINT>(std::round(w * scale)));
        dst_h = std::max<UINT>(1, static_cast<UINT>(std::round(h * scale)));
        ComPtr<IWICBitmapScaler> scaler;
        if (SUCCEEDED(worker->wic->CreateBitmapScaler(&scaler)) &&
            SUCCEEDED(scaler->Initialize(current.Get(), dst_w, dst_h, WICBitmapInterpolationModeFant))) {
            current = scaler;
        } else {
            dst_w = w;
            dst_h = h;
        }
    }

    size_t need = static_cast<size_t>(dst_w) * static_cast<size_t>(dst_h) * 4;
    if (need == 0 || need > static_cast<size_t>(max_bytes)) return 0;

    ComPtr<IWICFormatConverter> conv;
    if (FAILED(worker->wic->CreateFormatConverter(&conv))) return 0;
    if (FAILED(conv->Initialize(
            current.Get(),
            GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            nullptr,
            0.0,
            WICBitmapPaletteTypeMedianCut))) {
        return 0;
    }

    if (FAILED(conv->CopyPixels(nullptr, dst_w * 4, static_cast<UINT>(need), out_bgra))) {
        return 0;
    }
    if (out_w) *out_w = static_cast<int>(dst_w);
    if (out_h) *out_h = static_cast<int>(dst_h);
    return 1;
}

int d2d_create_bitmap_from_file_bytes(D2dCtx* ctx, const uint8_t* bytes, int len, int max_dim) {
    if (!ensure_wic(ctx) || !bytes || len <= 0) return 0;
    ComPtr<IWICStream> stream;
    if (FAILED(ctx->wic->CreateStream(&stream))) return 0;
    if (FAILED(stream->InitializeFromMemory(const_cast<BYTE*>(bytes), static_cast<DWORD>(len)))) return 0;
    ComPtr<IWICBitmapDecoder> dec;
    if (FAILED(ctx->wic->CreateDecoderFromStream(
            stream.Get(), nullptr, WICDecodeMetadataCacheOnLoad, &dec))) return 0;
    ComPtr<IWICBitmapFrameDecode> frame;
    if (FAILED(dec->GetFrame(0, &frame))) return 0;
    return register_wic_bitmap(ctx, frame.Get(), max_dim);
}

int d2d_create_bitmap_from_bgra(D2dCtx* ctx, int w, int h, const uint8_t* bgra) {
    if (!ctx || !ctx->d2d_ctx || !bgra || w <= 0 || h <= 0) return 0;
    ComPtr<ID2D1Bitmap1> bmp;
    D2D1_BITMAP_PROPERTIES1 props = D2D1::BitmapProperties1(
        D2D1_BITMAP_OPTIONS_NONE,
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
    if (FAILED(ctx->d2d_ctx->CreateBitmap(
            D2D1::SizeU(static_cast<UINT32>(w), static_cast<UINT32>(h)),
            bgra, static_cast<UINT32>(w * 4), &props, &bmp))) return 0;
    int id = ctx->next_bmp_id++;
    ctx->bitmaps.emplace(id, std::move(bmp));
    return id;
}

int d2d_get_bitmap_size(D2dCtx* ctx, int bmp_id, float* out_w, float* out_h) {
    if (!ctx || bmp_id <= 0) return 0;
    auto it = ctx->bitmaps.find(bmp_id);
    if (it == ctx->bitmaps.end() || !it->second) return 0;
    D2D1_SIZE_U sz = it->second->GetPixelSize();
    if (out_w) *out_w = static_cast<float>(sz.width);
    if (out_h) *out_h = static_cast<float>(sz.height);
    return 1;
}

void d2d_draw_bitmap(D2dCtx* ctx, int bmp_id, float x, float y, float w, float h, float alpha) {
    if (!ctx || !ctx->d2d_ctx || bmp_id <= 0 || alpha <= 0.002f) return;
    auto it = ctx->bitmaps.find(bmp_id);
    if (it == ctx->bitmaps.end() || !it->second) return;
    ctx->d2d_ctx->DrawBitmap(
        it->second.Get(),
        D2D1::RectF(x, y, x + w, y + h),
        std::clamp(alpha, 0.0f, 1.0f),
        D2D1_INTERPOLATION_MODE_LINEAR);
}

void d2d_destroy_bitmap(D2dCtx* ctx, int bmp_id) {
    if (!ctx || bmp_id <= 0) return;
    ctx->bitmaps.erase(bmp_id);
}

} // extern "C"
