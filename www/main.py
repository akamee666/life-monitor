from sanic import Sanic
from sanic.response import json

app = Sanic(name='my_app')
app.static("/","index.html")

@app.route("/get-something")
async def test(request):
    return json({"hello": "world"})

if __name__ == "__main__":
    app.run(host="0.0.0.0", port=8000)
