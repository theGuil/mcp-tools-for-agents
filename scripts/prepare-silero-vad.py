"""Prepara o modelo Silero VAD embutido em core/models/silero_vad_16k.onnx.

O ONNX oficial (silero_vad_16k_op15.onnx, do repositório snakers4/silero-vad,
MIT) tem nós `If` que só decidem forma de tensor; com entrada fixa (1 x 576
amostras, estado 2 x 1 x 128, 16 kHz) a condição é constante, mas o `tract`
não traduz `If`. Este script fixa as formas, avalia cada condição com o
onnxruntime, substitui o `If` pelo ramo escolhido e confere que a saída
continua idêntica à do original.

Uso (uma vez, só ao atualizar o modelo):
    pip install onnx onnxruntime numpy
    python3 scripts/prepare-silero-vad.py            # lê silero_vad_16k_op15.onnx na pasta atual
    cp silero_noif.onnx core/models/silero_vad_16k.onnx
"""
import onnx, numpy as np, onnxruntime as ort
from onnx import helper

def run_conds(model, names):
    m = onnx.ModelProto(); m.CopyFrom(model)
    existing = {o.name for o in m.graph.output}
    for n in names:
        if n not in existing:
            vi = onnx.ValueInfoProto(); vi.name = n; m.graph.output.append(vi)
    sess = ort.InferenceSession(m.SerializeToString(), providers=['CPUExecutionProvider'])
    feeds = {'input': np.zeros((1, 576), np.float32), 'state': np.zeros((2, 1, 128), np.float32)}
    outs = sess.run(list(names), feeds)
    return dict(zip(names, outs))

def inline_once(model):
    g = model.graph
    ifs = [n for n in g.node if n.op_type == 'If']
    if not ifs:
        return False
    conds = run_conds(model, [n.input[0] for n in ifs])
    new_nodes = []
    for n in g.node:
        if n.op_type != 'If':
            new_nodes.append(n); continue
        cond = bool(np.asarray(conds[n.input[0]]).reshape(-1)[0])
        attr = 'then_branch' if cond else 'else_branch'
        branch = [a for a in n.attribute if a.name == attr][0].g
        # Renomeia saídas do ramo para as saídas do If.
        rename = {bo.name: o for bo, o in zip(branch.output, n.output)}
        for bn in branch.node:
            bn2 = onnx.NodeProto(); bn2.CopyFrom(bn)
            bn2.name = n.name + '/' + bn.name
            for i, o in enumerate(bn2.output):
                if o in rename: bn2.output[i] = rename[o]
            for i, inp in enumerate(bn2.input):
                if inp in rename: bn2.input[i] = rename[inp]
            new_nodes.append(bn2)
        for init in branch.initializer:
            g.initializer.append(init)
        # Um ramo que só devolve um valor externo (Identity) já está tratado; um ramo
        # que devolve uma entrada direta sem nó precisa de Identity.
        for bo, o in zip(branch.output, n.output):
            if not any(o in bn.output for bn in new_nodes):
                new_nodes.append(helper.make_node('Identity', [bo.name], [o], name=n.name + '/id'))
    del g.node[:]
    g.node.extend(new_nodes)
    return True

# sr vira constante 16000; input e state ganham forma fixa.
m = onnx.load('silero_vad_16k_op15.onnx')
g = m.graph
from onnx import numpy_helper
sr = [i for i in g.input if i.name == 'sr'][0]
g.input.remove(sr)
g.initializer.append(numpy_helper.from_array(np.array(16000, dtype=np.int64), 'sr'))
for i in g.input:
    dims = i.type.tensor_type.shape.dim
    if i.name == 'input':
        dims[0].dim_value = 1
        dims[1].dim_value = 576
    if i.name == 'state':
        dims[1].dim_value = 1
rounds = 0
while inline_once(m):
    rounds += 1
print('rounds', rounds, 'If left', sum(1 for n in m.graph.node if n.op_type == 'If'))
m = onnx.shape_inference.infer_shapes(m)
onnx.checker.check_model(m)
onnx.save(m, 'silero_noif.onnx')
# Confere contra o original
ref = ort.InferenceSession('silero_vad_16k_op15.onnx', providers=['CPUExecutionProvider'])
new = ort.InferenceSession('silero_noif.onnx', providers=['CPUExecutionProvider'])
rng = np.random.default_rng(0)
x = rng.standard_normal((1, 576)).astype(np.float32) * 0.1
st = np.zeros((2, 1, 128), np.float32)
a = ref.run(None, {'input': x, 'state': st, 'sr': np.array(16000, np.int64)})
b = new.run(None, {'input': x, 'state': st})
print('diff output', abs(a[0] - b[0]).max(), 'diff state', abs(a[1] - b[1]).max(), 'ops', sorted(set(n.op_type for n in m.graph.node)))
