#include <OpenColorIO/OpenColorIO.h>
#include <string>
#include <exception>
#include <cstdint>
#include <vector>
#include <sstream>
namespace O = OCIO_NAMESPACE;
namespace {
thread_local std::string last_error;
struct Config { O::ConstConfigRcPtr value; };
struct Program { O::ConstConfigRcPtr config; O::ConstProcessorRcPtr processor; O::ConstCPUProcessorRcPtr cpu; O::GpuShaderDescRcPtr gpu; std::vector<std::string> files; };
}
extern "C" {
const char* vv_ocio_error() { return last_error.c_str(); }
void* vv_ocio_config(const char* path) {
    try {
        O::ClearAllCaches();
        auto c = std::string(path).rfind("ocio://",0)==0 ? O::Config::CreateFromBuiltinConfig(path) : O::Config::CreateFromFile(path);
        c->validate(); return new Config{c};
    } catch(const std::exception& e) {last_error=e.what();return nullptr;}
}
void vv_ocio_config_free(void* ptr) { delete static_cast<Config*>(ptr); }
int vv_ocio_count(void* ptr, int kind, const char* display) {
    try { auto c=static_cast<Config*>(ptr)->value;
        switch(kind) {case 0:return c->getNumColorSpaces();case 1:return c->getNumDisplays();case 2:return c->getNumViews(display);default:return 0;}
    } catch(const std::exception& e) {last_error=e.what();return -1;}
}
const char* vv_ocio_name(void* ptr,int kind,const char* display,int index) {
    try {auto c=static_cast<Config*>(ptr)->value;
        switch(kind) {case 0:return c->getColorSpaceNameByIndex(index);case 1:return c->getDisplay(index);case 2:return c->getView(display,index);default:return nullptr;}
    } catch(const std::exception& e) {last_error=e.what();return nullptr;}
}
void* vv_ocio_program(void* ptr,const char* source,const char* destination,const char* display,const char* view) {
    try {
        auto c=static_cast<Config*>(ptr)->value;
        O::ConstProcessorRcPtr p;
        if (*display) {
            auto t=O::DisplayViewTransform::Create();t->setSrc(source);t->setDisplay(display);t->setView(view);
            if (*destination) {
                std::string displaySpace=c->getDisplayViewColorSpaceName(display,view);
                if(displaySpace=="<USE_DISPLAY_NAME>") displaySpace=display;
                auto decode=O::ColorSpaceTransform::Create();decode->setSrc(displaySpace.c_str());decode->setDst(destination);
                auto group=O::GroupTransform::Create();group->appendTransform(t);group->appendTransform(decode);p=c->getProcessor(group);
            } else { p=c->getProcessor(t); }
        }
        else { p=c->getProcessor(source,destination); }
        auto gpu=O::GpuShaderDesc::CreateShaderDesc();
        gpu->setLanguage(O::GPU_LANGUAGE_GLSL_VK_4_6);gpu->setFunctionName("veylvik_color");gpu->setResourcePrefix("vv_");gpu->setDescriptorSetIndex(0,3);
        p->getDefaultGPUProcessor()->extractGpuShaderInfo(gpu);
        if(gpu->getNumUniforms()) {last_error="Dynamic OCIO uniforms are not supported; use a static processor";return nullptr;}
        std::vector<std::string> files;
        const auto metadata=p->getProcessorMetadata();
        for(int i=0;i<metadata->getNumFiles();++i) files.emplace_back(c->getCurrentContext()->resolveFileLocation(metadata->getFile(i)));
        return new Program{c,p,p->getDefaultCPUProcessor(),gpu,std::move(files)};
    }catch(const std::exception& e){last_error=e.what();return nullptr;}
}
// Resolve all file/context references into a lossless CTF, never an approximate bake.
const char* vv_ocio_portable(void* ptr, int config) {
    try {
        thread_local std::string result;
        auto p=static_cast<Program*>(ptr);
        std::ostringstream stream;
        if(config) {
            auto c=O::Config::Create(); c->setMajorVersion(2); c->setMinorVersion(0); c->setSearchPath(".");
            auto input=O::ColorSpace::Create(O::REFERENCE_SPACE_SCENE); input->setName("Input");
            auto output=O::ColorSpace::Create(O::REFERENCE_SPACE_SCENE); output->setName("Output");
            auto file=O::FileTransform::Create(); file->setSrc("transform.ctf");
            output->setTransform(file,O::COLORSPACE_DIR_FROM_REFERENCE);
            c->addColorSpace(input); c->addColorSpace(output); c->setRole(O::ROLE_DEFAULT,"Input");
            c->addDisplayView("Portable","Output","Output",""); c->serialize(stream);
        } else {
            const char* format=nullptr;
            for(int i=0;i<O::GroupTransform::GetNumWriteFormats();++i)
                if(std::string(O::GroupTransform::GetFormatExtensionByIndex(i))=="ctf") format=O::GroupTransform::GetFormatNameByIndex(i);
            if(!format) throw std::runtime_error("This OCIO build cannot write lossless CTF transforms");
            p->processor->createGroupTransform()->write(p->config,format,stream);
        }
        result=stream.str(); return result.c_str();
    } catch(const std::exception& e) {last_error=e.what();return nullptr;}
}
void vv_ocio_program_free(void* ptr){delete static_cast<Program*>(ptr);}
const char* vv_ocio_shader(void* ptr){return static_cast<Program*>(ptr)->gpu->getShaderText();}
int vv_ocio_files(void* ptr){return static_cast<int>(static_cast<Program*>(ptr)->files.size());}
const char* vv_ocio_file(void* ptr,int index){return static_cast<Program*>(ptr)->files.at(index).c_str();}
uint32_t vv_ocio_textures(void* ptr){auto g=static_cast<Program*>(ptr)->gpu;return g->getNumTextures()+g->getNum3DTextures();}
struct Texture {const char* sampler;const float* values;uint32_t width,height,depth,channels,dimension,linear,binding;};
int vv_ocio_texture(void* ptr,uint32_t index,Texture* out) {
    try {
        auto g=static_cast<Program*>(ptr)->gpu; const char* name=nullptr;O::Interpolation interpolation;
        if(index<g->getNumTextures()) {
            O::GpuShaderCreator::TextureType channels;O::GpuShaderCreator::TextureDimensions dimensions;
            g->getTexture(index,name,out->sampler,out->width,out->height,channels,dimensions,interpolation);
            g->getTextureValues(index,out->values);out->depth=1;
            out->channels=channels==O::GpuShaderCreator::TEXTURE_RED_CHANNEL?1:3;
            out->dimension=dimensions==O::GpuShaderCreator::TEXTURE_1D?1:2;
            out->binding=g->getTextureShaderBindingIndex(index);
        } else {
            index-=g->getNumTextures();
            g->get3DTexture(index,name,out->sampler,out->width,interpolation);out->height=out->width;out->depth=out->width;
            g->get3DTextureValues(index,out->values);out->channels=3;out->dimension=3;out->binding=g->get3DTextureShaderBindingIndex(index);
        }
        out->linear=interpolation!=O::INTERP_NEAREST;return 1;
    }catch(const std::exception& e){last_error=e.what();return 0;}
}
int vv_ocio_reference(void* ptr,float* rgba){try {static_cast<Program*>(ptr)->cpu->applyRGBA(rgba);return 1;}catch(const std::exception&e){last_error=e.what();return 0;}}
}
