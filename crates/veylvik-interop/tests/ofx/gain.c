/* A test OpenFX filter written from the standard's text, with its own declarations:
   output = source * gain, plus `lift` times (row / height) so a flipped picture shows,
   or with mode 1 a flat colour. Built by the host's tests with the system C compiler. */
#include <sys/prctl.h>
#include <stddef.h>
#include <string.h>
typedef int OfxStatus;
typedef void *H;
typedef struct OfxHost { H host; const void *(*fetchSuite)(H, const char *, int); } OfxHost;
typedef struct OfxPlugin {
  const char *pluginApi; int apiVersion; const char *pluginIdentifier;
  unsigned int major, minor; void (*setHost)(OfxHost *);
  OfxStatus (*mainEntry)(const char *, const void *, H, H);
} OfxPlugin;
typedef struct { OfxStatus (*setPointer)(H,const char*,int,void*); OfxStatus (*setString)(H,const char*,int,const char*);
  OfxStatus (*setDouble)(H,const char*,int,double); OfxStatus (*setInt)(H,const char*,int,int);
  OfxStatus (*setPointerN)(H,const char*,int,void*const*); OfxStatus (*setStringN)(H,const char*,int,const char*const*);
  OfxStatus (*setDoubleN)(H,const char*,int,const double*); OfxStatus (*setIntN)(H,const char*,int,const int*);
  OfxStatus (*getPointer)(H,const char*,int,void**); OfxStatus (*getString)(H,const char*,int,char**);
  OfxStatus (*getDouble)(H,const char*,int,double*); OfxStatus (*getInt)(H,const char*,int,int*);
  OfxStatus (*getPointerN)(H,const char*,int,void**); OfxStatus (*getStringN)(H,const char*,int,char**);
  OfxStatus (*getDoubleN)(H,const char*,int,double*); OfxStatus (*getIntN)(H,const char*,int,int*);
  OfxStatus (*reset)(H,const char*); OfxStatus (*getDimension)(H,const char*,int*); } PropSuite;
typedef struct OfxRectD { double x1, y1, x2, y2; } OfxRectD;
typedef struct { OfxStatus (*getPropertySet)(H,H*); OfxStatus (*getParamSet)(H,H*); OfxStatus (*clipDefine)(H,const char*,H*);
  OfxStatus (*clipGetHandle)(H,const char*,H*,H*); OfxStatus (*clipGetPropertySet)(H,H*);
  OfxStatus (*clipGetImage)(H,double,const OfxRectD*,H*); OfxStatus (*clipReleaseImage)(H);
  void *rest[6]; } EffectSuite;
typedef struct { OfxStatus (*paramDefine)(H,const char*,const char*,H*); OfxStatus (*paramGetHandle)(H,const char*,H*,H*);
  OfxStatus (*paramSetGetPropertySet)(H,H*); OfxStatus (*paramGetPropertySet)(H,H*);
  OfxStatus (*paramGetValue)(H,...); OfxStatus (*paramGetValueAtTime)(H,double,...);
  OfxStatus (*paramGetDerivative)(H,double,...); OfxStatus (*paramGetIntegral)(H,double,double,...);
  OfxStatus (*paramSetValue)(H,...); OfxStatus (*paramSetValueAtTime)(H,double,...); void *rest[8]; } ParamSuite;
typedef struct { OfxStatus (*message)(H,const char*,const char*,const char*,...); } MessageSuite;
typedef struct { OfxStatus (*getValue)(H,int,double,double,double*); OfxStatus (*getCount)(H,int,double,int*);
  OfxStatus (*getNth)(H,int,double,int,double*,double*); OfxStatus (*setNth)(H,int,double,int,double,double,int);
  OfxStatus (*add)(H,int,double,double,double,int); void *rest[2]; } CurveSuite;
static OfxHost *host; static PropSuite *props; static EffectSuite *effects; static ParamSuite *params; static MessageSuite *messages; static CurveSuite *curves;
static void setHost(OfxHost *h) { host = h; }
static OfxStatus mainEntry(const char *action, const void *handle, H in, H out) {
  H effect = (H)handle, p, ps, param;
  (void)out;
  if (!strcmp(action, "OfxActionLoad")) {
    props = (PropSuite *)host->fetchSuite(host->host, "OfxPropertySuite", 1);
    effects = (EffectSuite *)host->fetchSuite(host->host, "OfxImageEffectSuite", 1);
    params = (ParamSuite *)host->fetchSuite(host->host, "OfxParameterSuite", 1);
    messages = (MessageSuite *)host->fetchSuite(host->host, "OfxMessageSuite", 1);
    curves = (CurveSuite *)host->fetchSuite(host->host, "OfxParametricParameterSuite", 1);
    return props && effects && params && messages ? 0 : 4;
  }
  if (!strcmp(action, "OfxActionDescribe")) {
    effects->getPropertySet(effect, &p);
    props->setString(p, "OfxPropLabel", 0, "Test gain");
    props->setString(p, "OfxImageEffectPluginPropGrouping", 0, "Veylvik tests");
    props->setString(p, "OfxImageEffectPropSupportedContexts", 0, "OfxImageEffectContextFilter");
    props->setString(p, "OfxImageEffectPropSupportedPixelDepths", 0, "OfxBitDepthFloat");
    props->setInt(p, "OfxImageEffectPropTemporalClipAccess", 0, 1);
    return 0;
  }
  if (!strcmp(action, "OfxImageEffectActionGetFramesNeeded")) {
    /* Mode 4 shows the previous frame: it needs the source one frame back. */
    double t = 0; int mode = 0;
    props->getDouble(in, "OfxPropTime", 0, &t);
    effects->getParamSet(effect, &ps);
    params->paramGetHandle(ps, "mode", &param, 0); params->paramGetValue(param, &mode);
    if (mode != 4) return 14;
    double range[2] = {t - 1.0, t - 1.0};
    props->setDoubleN(out, "OfxImageClipPropFrameRange_Source", 2, range);
    return 0;
  }
  if (!strcmp(action, "OfxImageEffectActionDescribeInContext")) {
    char *context = 0;
    props->getString(in, "OfxImageEffectPropContext", 0, &context);
    if (!context || strcmp(context, "OfxImageEffectContextFilter")) return 5;
    effects->clipDefine(effect, "Source", &p);
    props->setString(p, "OfxImageEffectPropSupportedComponents", 0, "OfxImageComponentRGBA");
    effects->clipDefine(effect, "Output", &p);
    props->setString(p, "OfxImageEffectPropSupportedComponents", 0, "OfxImageComponentRGBA");
    effects->getParamSet(effect, &ps);
    params->paramDefine(ps, "OfxParamTypeDouble", "gain", &p);
    props->setDouble(p, "OfxParamPropDefault", 0, 1.0);
    props->setDouble(p, "OfxParamPropDisplayMin", 0, 0.0);
    props->setDouble(p, "OfxParamPropDisplayMax", 0, 4.0);
    props->setString(p, "OfxPropLabel", 0, "Gain");
    params->paramDefine(ps, "OfxParamTypeDouble", "lift", &p);
    params->paramDefine(ps, "OfxParamTypeChoice", "mode", &p);
    props->setString(p, "OfxParamPropChoiceOption", 0, "Multiply");
    props->setString(p, "OfxParamPropChoiceOption", 1, "Flat colour");
    params->paramDefine(ps, "OfxParamTypeRGBA", "colour", &p);
    { double c[4] = {0.25, 0.5, 0.75, 1.0}; props->setDoubleN(p, "OfxParamPropDefault", 4, c); }
    params->paramDefine(ps, "OfxParamTypeString", "note", &p);
    props->setString(p, "OfxParamPropDefault", 0, "hello");
    params->paramDefine(ps, "OfxParamTypeInteger", "steps", &p);
    params->paramDefine(ps, "OfxParamTypeDouble2D", "centre", &p);
    params->paramDefine(ps, "OfxParamTypeString", "written", &p);
    /* A curve: default points (0, 0) and (1, 0.5), added to the descriptor. */
    params->paramDefine(ps, "OfxParamTypeParametric", "curve", &p);
    props->setInt(p, "OfxParamPropParametricDimension", 0, 1);
    props->setString(p, "OfxParamPropDimensionLabel", 0, "master");
    params->paramGetHandle(ps, "curve", &param, 0);
    curves->add(param, 0, 0.0, 0.0, 0.0, 0);
    curves->add(param, 0, 0.0, 1.0, 0.5, 0);
    return 0;
  }
  if (!strcmp(action, "OfxActionCreateInstance")) {
    /* Values a plugin writes itself, through the variadic setter: a double, an int,
       two doubles and a string. */
    effects->getParamSet(effect, &ps);
    params->paramGetHandle(ps, "lift", &param, 0); params->paramSetValue(param, 0.125);
    params->paramGetHandle(ps, "steps", &param, 0); params->paramSetValue(param, 7);
    params->paramGetHandle(ps, "centre", &param, 0); params->paramSetValue(param, 1.5, -2.5);
    params->paramGetHandle(ps, "written", &param, 0); params->paramSetValue(param, "by the plugin");
    return 0;
  }
  if (!strcmp(action, "OfxImageEffectActionRender")) {
    double gain = 0, lift = 0, c[4] = {0}; int mode = 0; char *note = 0;
    H src, dst, simg = 0, dimg; float *sp = 0, *dp; int b[4], rows, rowsS = 0;
    effects->getParamSet(effect, &ps);
    params->paramGetHandle(ps, "gain", &param, 0); params->paramGetValue(param, &gain);
    params->paramGetHandle(ps, "lift", &param, 0); params->paramGetValue(param, &lift);
    params->paramGetHandle(ps, "mode", &param, 0); params->paramGetValue(param, &mode);
    params->paramGetHandle(ps, "colour", &param, 0); params->paramGetValue(param, &c[0], &c[1], &c[2], &c[3]);
    params->paramGetHandle(ps, "note", &param, 0); params->paramGetValue(param, &note);
    if (!note || strcmp(note, "hello")) messages->message(effect, "OfxMessageError", 0, "note was not hello");
    if (mode == 2) {
      /* A plugin that crashes: a real SIGSEGV, but the helper is made undumpable first so
         the system keeps no core of it and shows no crash notice for a test that wants it. */
      prctl(PR_SET_DUMPABLE, 0, 0, 0, 0);
      volatile int *nowhere = 0; *nowhere = 1;
    }
    effects->clipGetHandle(effect, "Output", &dst, 0);
    double now = 0;
    props->getDouble(in, "OfxPropTime", 0, &now);
    if (effects->clipGetImage(dst, now, 0, &dimg)) return 1;
    props->getPointer(dimg, "OfxImagePropData", 0, (void **)&dp);
    props->getIntN(dimg, "OfxImagePropBounds", 4, b);
    props->getInt(dimg, "OfxImagePropRowBytes", 0, &rows);
    if (mode == 0 || mode == 3 || mode == 4) {
      effects->clipGetHandle(effect, "Source", &src, 0);
      if (effects->clipGetImage(src, mode == 4 ? now - 1.0 : now, 0, &simg)) { messages->message(effect, "OfxMessageError", 0, "%s", "no source, as the support library says it"); return 1; }
      props->getPointer(simg, "OfxImagePropData", 0, (void **)&sp);
      props->getInt(simg, "OfxImagePropRowBytes", 0, &rowsS);
    }
    for (int y = b[1]; y < b[3]; y++) {
      float *d = (float *)((char *)dp + (size_t)(y - b[1]) * rows);
      float *s = sp ? (float *)((char *)sp + (size_t)(y - b[1]) * rowsS) : 0;
      double up = lift * (double)y / (double)(b[3] - b[1]);
      for (int x = 0; x < b[2] - b[0]; x++)
        for (int k = 0; k < 4; k++)
          d[x * 4 + k] = mode == 4 ? s[x * 4 + k] : mode == 0 ? (float)(s[x * 4 + k] * gain + (k < 3 ? up : 0.0)) : (float)c[k];
    if (mode == 3) {
      /* The curve applied to the source: read through the host's evaluation. */
      H curve; double v;
      params->paramGetHandle(ps, "curve", &curve, 0);
      for (int y = b[1]; y < b[3]; y++) {
        float *d = (float *)((char *)dp + (size_t)(y - b[1]) * rows);
        float *s = (float *)((char *)sp + (size_t)(y - b[1]) * rowsS);
        for (int x = 0; x < (b[2] - b[0]) * 4; x++) { curves->getValue(curve, 0, 0.0, s[x], &v); d[x] = (float)v; }
      }
    }
    }
    if (simg) effects->clipReleaseImage(simg);
    effects->clipReleaseImage(dimg);
    return 0;
  }
  return 14; /* kOfxStatReplyDefault */
}
static OfxPlugin plugin = {"OfxImageEffectPluginAPI", 1, "org.veylvik.test.gain", 1, 0, setHost, mainEntry};
int OfxGetNumberOfPlugins(void) { return 1; }
OfxPlugin *OfxGetPlugin(int n) { return n == 0 ? &plugin : 0; }
